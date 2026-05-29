//! Model (`.vmdl_c`) orchestration: open a VPK, find compiled models, and hand
//! their bytes to `morphic` for decode. Mirrors [`crate::portrait`]. Today it
//! exposes [`inspect_models`] (a structural read); glTF export lands later
//! (see `docs/vmdl-glb-exporter.md`).

use anyhow::{Context, Result};
use std::path::Path;

pub use morphic::model::{BlockSummary, ModelInfo, VertexTarget};

/// Default candidate clips for a bare `--pose`, in priority order. Menu-pose
/// naming is not uniform across Deadlock heroes (Vindicta uses `ui_hero_pose`,
/// Abrams/McGinnis/Haze use `hero_pose` / `hero_roster_pose` / `hero_roster_ready`),
/// so try the curated roster poses first, then fall back to progressively more
/// generic idles, so any hero bakes a sensible still.
pub const DEFAULT_POSE_CLIPS: [&str; 7] = [
    "ui_hero_pose",
    "hero_pose",
    "hero_roster_pose",
    "hero_roster_ready",
    "ui_hero_select",
    "idle_loadout",
    "primary_stand_idle",
];

/// Animation-clip selection for model export. By default every clip the model
/// carries is exported; these narrow that.
#[derive(Debug, Clone, Default)]
pub struct AnimOptions {
    /// Drop all animation clips (export the static mesh + skeleton only).
    pub no_anim: bool,
    /// When non-empty, keep only clips whose name appears here. Ignored if
    /// [`AnimOptions::no_anim`] is set (no-anim wins).
    pub clips: Vec<String>,
    /// Bake a single frame into the mesh as static geometry. When set it wins
    /// over [`AnimOptions::no_anim`] and [`AnimOptions::clips`]: the output is a
    /// plain posed mesh with no skeleton, skin, or clips.
    pub pose: Option<PoseSelection>,
}

/// A single-frame pose to bake into the mesh (see [`morphic::model::bake_pose`]).
#[derive(Debug, Clone, Default)]
pub struct PoseSelection {
    /// Candidate clip names in priority order; the first the model carries wins.
    /// Empty means use [`DEFAULT_POSE_CLIPS`].
    pub clips: Vec<String>,
    /// Frame index to sample (clamped to the clip's range).
    pub frame: usize,
}

impl AnimOptions {
    /// Applies the selection to a decoded model's clip list in place.
    fn apply(&self, clips: &mut Vec<morphic::model::Clip>) {
        if self.no_anim {
            clips.clear();
        } else if !self.clips.is_empty() {
            clips.retain(|c| self.clips.iter().any(|w| w == &c.name));
        }
    }
}

/// Resolves compiled resource paths (`.vmat_c`, `.vtex_c`) across the open VPKs
/// in order: the skin VPK first, then the base `pak01_dir.vpk`. Skins embed
/// their geometry but reference materials/textures that may live in the base
/// pak, so the model exporter needs both. Implements [`morphic::model::FileResolver`]
/// to keep `morphic` free of VPK I/O.
struct VpkResolver {
    vpks: Vec<valve_pak::VPK>,
}

impl morphic::model::FileResolver for VpkResolver {
    fn resolve(&self, compiled_path: &str) -> Option<Vec<u8>> {
        for vpk in &self.vpks {
            if let Ok(mut vf) = vpk.get_file(compiled_path) {
                if let Ok(bytes) = vf.read_all() {
                    return Some(bytes);
                }
            }
        }
        None
    }
}

/// Decode a `.vmdl_c` at an explicit VPK `entry` path and write it as a textured
/// binary glTF.
///
/// `vpk` is searched first, then `base` (the base `pak01_dir.vpk`), for both the
/// model entry and its materials/textures. A mesh skin ships its own model in
/// `vpk`; a texture-only skin ships no model, so the entry is read from `base`
/// while the skin's overriding textures (in `vpk`) still win on resolution.
pub fn export_model(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    anim: &AnimOptions,
    out: impl AsRef<Path>,
) -> Result<()> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    if read_entry(&vpks, entry).is_none() {
        anyhow::bail!("model entry {entry} not found in the given VPK(s)");
    }
    export_resolved(vpks, entry, anim, out.as_ref())
}

/// Like [`export_model`] but discovers the hero's body model by codename instead
/// of an explicit entry path: the first `.vmdl_c` under a `models/heroes*`
/// directory whose file name is exactly `<codename>.vmdl_c` (so `hornet.vmdl_c`,
/// not `hornet_backup.vmdl_c` or `hornet_lod1`, and not weapon/prop sub-meshes
/// like `bookworm_sword`). The given VPK is searched first (a mesh skin ships its
/// own model), then the base pak (texture-only skins reuse the base mesh).
pub fn export_hero_model(
    vpk: impl AsRef<Path>,
    codename: &str,
    base: Option<&Path>,
    anim: &AnimOptions,
    out: impl AsRef<Path>,
) -> Result<()> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let entry = discover_hero_entry(&vpks, codename).with_context(|| {
        format!("no body model (`<dir>/{codename}.vmdl_c` under models/heroes*) found in the given VPK(s)")
    })?;
    export_resolved(vpks, &entry, anim, out.as_ref())
}

/// Opens the VPKs in resolution priority order: `vpk` first (a skin's overrides
/// win), then the base pak.
fn open_vpks(vpk: &Path, base: Option<&Path>) -> Result<Vec<valve_pak::VPK>> {
    let mut vpks =
        vec![valve_pak::open(vpk).with_context(|| format!("opening {}", vpk.display()))?];
    if let Some(base) = base {
        vpks.push(valve_pak::open(base).with_context(|| format!("opening {}", base.display()))?);
    }
    Ok(vpks)
}

/// Reads `entry`, decodes it, textures it via the cross-VPK resolver, and writes
/// the `.glb`. Consumes `vpks` (they move into the resolver).
fn export_resolved(
    vpks: Vec<valve_pak::VPK>,
    entry: &str,
    anim: &AnimOptions,
    out: &Path,
) -> Result<()> {
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    let mut model = morphic::model::decode(&bytes).with_context(|| format!("decoding {entry}"))?;

    if let Some(pose) = &anim.pose {
        let candidates: Vec<&str> = if pose.clips.is_empty() {
            DEFAULT_POSE_CLIPS.to_vec()
        } else {
            pose.clips.iter().map(String::as_str).collect()
        };
        // Skin mods ship the mesh + rig but no animation clips (those live in the
        // base game). When this model carries none of the candidate clips, source
        // them from the same entry in the base pak (vpks after the first) and map
        // by bone name. Same hero, same rig, so no cross-hero retargeting.
        let has_own_clip = candidates.iter().any(|c| {
            model
                .animations
                .iter()
                .any(|a| a.name.eq_ignore_ascii_case(c))
        });
        model = if has_own_clip {
            morphic::model::bake_pose(&model, &candidates, pose.frame)
        } else if let Some(donor_bytes) = read_entry(&vpks[1..], entry) {
            let donor = morphic::model::decode(&donor_bytes)
                .with_context(|| format!("decoding base clips for {entry}"))?;
            morphic::model::bake_pose_from(&model, &donor, &candidates, pose.frame)
        } else {
            morphic::model::bake_pose(&model, &candidates, pose.frame)
        };
    } else {
        anim.apply(&mut model.animations);
    }

    let resolver = VpkResolver { vpks };
    let glb = morphic::model::to_glb_textured(&model, &resolver)
        .with_context(|| format!("writing glb for {entry}"))?;

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(out, &glb).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Finds the hero body model entry for `codename` across `vpks` (first match
/// wins). The body model's file name is exactly `<codename>.vmdl_c` and it lives
/// under a `models/heroes...` directory.
fn discover_hero_entry(vpks: &[valve_pak::VPK], codename: &str) -> Option<String> {
    let want = format!("{codename}.vmdl_c");
    for vpk in vpks {
        for path in vpk.file_paths() {
            let basename = path.rsplit('/').next().unwrap_or(path);
            if basename == want && path.contains("/heroes") {
                return Some(path.clone());
            }
        }
    }
    None
}

/// A geometric transform applied to a model's editable vertex buffers (Tier-0
/// displacement: reshape existing geometry, topology unchanged). Scale is uniform
/// about each buffer's centroid; translation is applied after, in model space.
#[derive(Debug, Clone)]
pub struct GeometryEdit {
    /// Edit only the mesh part with this exact name (e.g. `body`, `gun`); `None`
    /// edits every displacement-editable buffer in the model.
    pub part: Option<String>,
    /// Uniform scale about each buffer's centroid. `1.0` leaves size unchanged.
    pub scale: f32,
    /// Translation in model space, applied after scaling.
    pub translate: [f32; 3],
}

impl Default for GeometryEdit {
    fn default() -> Self {
        Self {
            part: None,
            scale: 1.0,
            translate: [0.0; 3],
        }
    }
}

/// What an [`edit_model_geometry`] run touched.
#[derive(Debug, Clone)]
pub struct GeometryEditReport {
    pub entry: String,
    pub vpk_entry: String,
    /// Names of the mesh parts whose geometry was moved.
    pub edited_parts: Vec<String>,
    pub edited_buffers: usize,
    pub edited_vertices: usize,
}

/// Lists the vertex buffers in a model entry (which mesh parts can be edited, and
/// the block index / vertex count of each). The CLI uses this for `--list` and to
/// resolve a `--part` name to a block index.
pub fn model_vertex_targets(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
) -> Result<Vec<VertexTarget>> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    morphic::model::vertex_targets(&bytes).with_context(|| format!("reading targets for {entry}"))
}

/// Applies a geometric transform to a model's editable vertex buffers and packs
/// the edited `.vmdl_c` into a standalone addon VPK at `vpk_entry` (defaulting to
/// `entry`, so it overrides the base pak). The geometry analog of
/// `soundevents --encode-vpk`: read from a VPK, edit, re-encode, pack.
///
/// Note: the model's stored bounds (`MDAT` per-mesh AABB) are not recomputed, so
/// large transforms can drift culling bounds; small reshapes are unaffected.
pub fn edit_model_geometry(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    edit: &GeometryEdit,
    out_vpk: impl AsRef<Path>,
    vpk_entry: Option<&str>,
) -> Result<GeometryEditReport> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;

    let targets = morphic::model::vertex_targets(&bytes)
        .with_context(|| format!("reading vertex targets for {entry}"))?;

    // Select the buffers to edit: editable, and matching --part if given.
    let selected: Vec<&VertexTarget> = targets
        .iter()
        .filter(|t| t.editable)
        .filter(|t| edit.part.as_deref().is_none_or(|p| t.mesh_name == p))
        .collect();

    if let Some(part) = &edit.part {
        if !targets.iter().any(|t| &t.mesh_name == part) {
            anyhow::bail!(
                "no mesh part named {part:?} in {entry} (parts: {})",
                part_list(&targets)
            );
        }
    }
    if selected.is_empty() {
        anyhow::bail!(
            "no displacement-editable vertex buffer to edit in {entry} \
             (need meshopt-compressed geometry with a float POSITION)"
        );
    }

    let mut edited_parts = Vec::new();
    let mut edited_vertices = 0usize;
    let edited_buffers = selected.len();
    for t in &selected {
        let positions = morphic::model::read_vertex_positions(&bytes, t.block_index)
            .with_context(|| format!("reading positions for {} buffer", t.mesh_name))?;
        let moved = apply_transform(&positions, edit);
        bytes = morphic::model::replace_vertex_positions(&bytes, t.block_index, &moved)
            .with_context(|| format!("splicing {} buffer", t.mesh_name))?;
        edited_vertices += positions.len();
        if !edited_parts.contains(&t.mesh_name) {
            edited_parts.push(t.mesh_name.clone());
        }
    }

    let vpk_entry = vpk_entry.unwrap_or(entry).to_string();
    crate::pack(&[(vpk_entry.as_str(), bytes.as_slice())], out_vpk.as_ref())
        .with_context(|| format!("packing edited model into {}", out_vpk.as_ref().display()))?;

    Ok(GeometryEditReport {
        entry: entry.to_string(),
        vpk_entry,
        edited_parts,
        edited_buffers,
        edited_vertices,
    })
}

/// Scale about the centroid, then translate.
// The vertex count -> f32 cast is for an averaging divisor; precision loss at
// realistic mesh sizes does not meaningfully move the centroid.
#[allow(clippy::cast_precision_loss)]
fn apply_transform(positions: &[[f32; 3]], edit: &GeometryEdit) -> Vec<[f32; 3]> {
    if positions.is_empty() {
        return Vec::new();
    }
    let n = positions.len() as f32;
    let mut centroid = [0.0f32; 3];
    for p in positions {
        for k in 0..3 {
            centroid[k] += p[k];
        }
    }
    for c in &mut centroid {
        *c /= n;
    }
    positions
        .iter()
        .map(|p| {
            [
                (p[0] - centroid[0]) * edit.scale + centroid[0] + edit.translate[0],
                (p[1] - centroid[1]) * edit.scale + centroid[1] + edit.translate[1],
                (p[2] - centroid[2]) * edit.scale + centroid[2] + edit.translate[2],
            ]
        })
        .collect()
}

fn part_list(targets: &[VertexTarget]) -> String {
    let mut names: Vec<&str> = targets.iter().map(|t| t.mesh_name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names.join(", ")
}

/// Reads a VPK entry from the first of `vpks` that contains it.
fn read_entry(vpks: &[valve_pak::VPK], entry: &str) -> Option<Vec<u8>> {
    for vpk in vpks {
        if let Ok(mut vf) = vpk.get_file(entry) {
            if let Ok(bytes) = vf.read_all() {
                return Some(bytes);
            }
        }
    }
    None
}

/// A compiled model found inside a VPK, with its structural summary.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    /// VPK-internal path (e.g. `models/heroes_staging/hornet_v3/hornet.vmdl_c`).
    pub path: String,
    pub info: ModelInfo,
}

/// Find every `.vmdl_c` in a VPK and summarize its block structure.
pub fn inspect_models(vpk_path: impl AsRef<Path>) -> Result<Vec<ModelEntry>> {
    let vpk_path = vpk_path.as_ref();
    let vpk =
        valve_pak::open(vpk_path).with_context(|| format!("opening {}", vpk_path.display()))?;

    let paths: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vmdl_c"))
        .cloned()
        .collect();

    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let mut vf = vpk
            .get_file(&path)
            .with_context(|| format!("locating {path}"))?;
        let bytes = vf.read_all().with_context(|| format!("reading {path}"))?;
        let info = morphic::model::inspect(&bytes).with_context(|| format!("parsing {path}"))?;
        out.push(ModelEntry { path, info });
    }

    Ok(out)
}
