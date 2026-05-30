//! Model (`.vmdl_c`) orchestration: open a VPK, find compiled models, and hand
//! their bytes to `morphic` for decode. Mirrors [`crate::portrait`]. Today it
//! exposes [`inspect_models`] (a structural read); glTF export lands later
//! (see `docs/vmdl-glb-exporter.md`).

use anyhow::{Context, Result};
use std::path::Path;

pub use morphic::model::{
    BlockSummary, DrawCallInfo, ModelInfo, RemovedDrawCall, ReplacedMeshPart, VertexTarget,
};

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
    /// Error out instead of falling back to the static bind pose when no posed
    /// frame is reachable: neither the model nor its base-pak donor carries a
    /// candidate clip, and no loose NM pose clip resolves. Lets a caller tell
    /// "posed" from "would be an unposed bind/T-pose still" and fall back to a 2D
    /// portrait. WIP heroes (`models/heroes_wip/...`) embed no clips in the
    /// `.vmdl_c`; their menu pose is recovered from a loose NM clip
    /// (`clips/*.vnmclip_c`) when present, else they bind-pose.
    pub require: bool,
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
        let carries_clip = |m: &morphic::model::Model| {
            candidates
                .iter()
                .any(|c| m.animations.iter().any(|a| a.name.eq_ignore_ascii_case(c)))
        };
        let has_own_clip = carries_clip(&model);
        // Decode the base-pak donor once (only when needed): it supplies the menu
        // clip for a clipless skin, and lets --require-pose tell whether a real
        // pose is reachable before baking.
        let donor = if has_own_clip {
            None
        } else {
            read_entry(&vpks[1..], entry)
                .map(|b| {
                    morphic::model::decode(&b)
                        .with_context(|| format!("decoding base clips for {entry}"))
                })
                .transpose()?
        };
        let donor_has_clip = donor.as_ref().is_some_and(carries_clip);
        // Newer WIP heroes embed no clips and have no clip-carrying donor (their
        // base entry is the same clipless model): their menu/idle pose ships as a
        // loose NM clip (`clips/<name>.vnmclip_c` + `<h>.vnmskel_c`) behind an
        // animation graph. Resolve and bake that static single frame directly.
        // It wins over a clipless donor but never over a real embedded/donor clip.
        let nm_posed = if has_own_clip || donor_has_clip {
            None
        } else {
            bake_loose_nm_pose(&vpks, entry, &model, &candidates)
        };
        if pose.require {
            let has_clip = has_own_clip || donor_has_clip || nm_posed.is_some();
            let has_skeleton = !model.skeleton.bones.is_empty();
            if !has_skeleton || !has_clip {
                anyhow::bail!(
                    "{entry} carries no menu/idle pose clip ({}); only a static bind \
                     pose is available and --require-pose was set",
                    candidates.join(", ")
                );
            }
        }
        model = if has_own_clip {
            morphic::model::bake_pose(&model, &candidates, pose.frame)
        } else if let Some(nm) = nm_posed {
            nm
        } else if let Some(donor) = &donor {
            morphic::model::bake_pose_from(&model, donor, &candidates, pose.frame)
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

/// Highest `_vN` suffix across a path's segments (`inferno_v4` -> 4,
/// `hornet_v3` -> 3), or 0 when no segment is versioned. Picks the current model
/// dir when several versions ship side by side.
fn version_rank(path: &str) -> u32 {
    path.split('/')
        .filter_map(|seg| {
            seg.rsplit_once("_v")
                .and_then(|(_, n)| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0)
}

/// Finds the hero body model entry for `codename`. The body model's file name is
/// exactly `<codename>.vmdl_c` and it lives under a `models/heroes...` directory.
///
/// VPKs are searched in priority order, so a mesh skin's own model (`vpks[0]`)
/// wins over the base pak. Within one VPK a hero can ship several matching
/// `<codename>.vmdl_c` (e.g. an old `heroes_wip/inferno` beside the current
/// `inferno_v4`). `valve_pak::file_paths()` iterates a `HashMap` (random order),
/// so taking the first match picks an arbitrary version per run; instead pick
/// deterministically: prefer a non-`heroes_wip` dir, then the highest `_vN`,
/// then the lexicographically greatest path.
fn discover_hero_entry(vpks: &[valve_pak::VPK], codename: &str) -> Option<String> {
    let want = format!("{codename}.vmdl_c");
    for vpk in vpks {
        let mut matches: Vec<&String> = vpk
            .file_paths()
            .filter(|p| {
                p.rsplit('/').next().unwrap_or(p.as_str()) == want.as_str() && p.contains("/heroes")
            })
            .collect();
        if matches.is_empty() {
            continue;
        }
        let key = |p: &str| (!p.contains("/heroes_wip/"), version_rank(p));
        matches.sort_by(|a, b| {
            key(b.as_str())
                .cmp(&key(a.as_str()))
                .then_with(|| b.as_str().cmp(a.as_str()))
        });
        return matches.first().map(|p| (*p).clone());
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

    // Read every selected buffer's positions first, then scale them all about a
    // single shared centroid. Scaling each buffer about its own centroid would
    // pull a multi-buffer part (hornet's body is two buffers) or a whole-model
    // edit apart; one shared centroid keeps the selection rigid as it grows.
    let mut buffers: Vec<(usize, &str, Vec<[f32; 3]>)> = Vec::with_capacity(selected.len());
    for t in &selected {
        let positions = morphic::model::read_vertex_positions(&bytes, t.block_index)
            .with_context(|| format!("reading positions for {} buffer", t.mesh_name))?;
        buffers.push((t.block_index, t.mesh_name.as_str(), positions));
    }
    let centroid = shared_centroid(buffers.iter().map(|(_, _, p)| p.as_slice()));

    let mut edited_parts: Vec<String> = Vec::new();
    let mut edited_vertices = 0usize;
    let edited_buffers = buffers.len();
    for (block_index, mesh_name, positions) in &buffers {
        let moved = transform_about(positions, centroid, edit);
        bytes = morphic::model::replace_vertex_positions(&bytes, *block_index, &moved)
            .with_context(|| format!("splicing {mesh_name} buffer"))?;
        edited_vertices += positions.len();
        if !edited_parts.iter().any(|n| n == mesh_name) {
            edited_parts.push((*mesh_name).to_string());
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

/// One model recolored by [`recolor_models_to_addon`].
#[derive(Debug, Clone)]
pub struct ModelRecolorEntry {
    pub entry: String,
    pub stats: crate::recolor::ModelRecolorStats,
}

/// Recolor the baked per-vertex colors of one or more model entries (read from
/// `vpk`, then `base`) to `recolor`, packing them all into one addon VPK at
/// `out`, each at its own entry path so it overrides the base pak in place. The
/// model analog of the multi-entry `vpkmerge texture` recolor (the third VFX
/// recolor mechanism, alongside particle params and texture hue).
///
/// Returns a per-entry report. Errors if an entry is missing, or carries no
/// color-bearing vertex buffer (a likely wrong-model mistake) so a silent
/// no-op addon is never written.
pub fn recolor_models_to_addon(
    vpk: impl AsRef<Path>,
    entries: &[String],
    base: Option<&Path>,
    recolor: crate::recolor::Recolor,
    out: impl AsRef<Path>,
) -> Result<Vec<ModelRecolorEntry>> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut packed: Vec<(String, Vec<u8>)> = Vec::with_capacity(entries.len());
    let mut report = Vec::with_capacity(entries.len());
    for entry in entries {
        let bytes =
            read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
        let (recolored, stats) = crate::recolor::recolor_model_vertex_colors(&bytes, recolor)
            .with_context(|| format!("recoloring {entry}"))?;
        if stats.buffers_recolored == 0 {
            anyhow::bail!("{entry} has no color-bearing vertex buffer to recolor (wrong model?)");
        }
        packed.push((entry.clone(), recolored));
        report.push(ModelRecolorEntry {
            entry: entry.clone(),
            stats,
        });
    }
    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    crate::pack(&refs, out.as_ref())
        .with_context(|| format!("packing recolored models into {}", out.as_ref().display()))?;
    Ok(report)
}

/// Exports one vertex buffer (by global block index) of a model to a `.glb` for
/// reshaping in Blender: a single triangle mesh carrying a `_ORIGID` per-vertex
/// attribute that maps the edit back to the original buffer (see
/// [`apply_model_edit_glb`]). The model is read from `vpk` (then `base`).
pub fn export_model_buffer_glb(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    block_index: usize,
    out_glb: impl AsRef<Path>,
) -> Result<()> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    let glb = morphic::model::export_buffer_for_edit(&bytes, block_index)
        .with_context(|| format!("exporting buffer at block {block_index} of {entry}"))?;
    let out = out_glb.as_ref();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(out, &glb).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Applies a Blender-reshaped `.glb` (from [`export_model_buffer_glb`]) back onto
/// a model: maps the edited positions to the original buffer via `_ORIGID`,
/// splices, and packs a standalone addon VPK at `vpk_entry` (default `entry`).
pub fn apply_model_edit_glb(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    block_index: usize,
    glb_bytes: &[u8],
    out_vpk: impl AsRef<Path>,
    vpk_entry: Option<&str>,
) -> Result<String> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    let edited = morphic::model::apply_edited_glb(&bytes, block_index, glb_bytes)
        .with_context(|| format!("applying edited glb to block {block_index} of {entry}"))?;
    let vpk_entry = vpk_entry.unwrap_or(entry).to_string();
    crate::pack(&[(vpk_entry.as_str(), edited.as_slice())], out_vpk.as_ref())
        .with_context(|| format!("packing edited model into {}", out_vpk.as_ref().display()))?;
    Ok(vpk_entry)
}

/// What a [`remove_model_material`] run dropped and where it was packed.
#[derive(Debug, Clone)]
pub struct MaterialRemovalReport {
    pub entry: String,
    pub vpk_entry: String,
    /// Each draw call removed (its mesh, material, and vertex/index counts).
    pub removed: Vec<RemovedDrawCall>,
}

/// Lists a model's renderable (LOD0) draw calls: the mesh part, the material each
/// renders, and the vertex/index counts. The CLI uses this for `--list-drawcalls`
/// so a user can find the exact material string to pass to
/// [`remove_model_material`].
pub fn model_draw_call_targets(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
) -> Result<Vec<DrawCallInfo>> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    morphic::model::draw_call_targets(&bytes)
        .with_context(|| format!("reading draw calls for {entry}"))
}

/// Removes every draw call whose material contains `material` (case-insensitive
/// substring) from the model, across all LODs, and packs the edited `.vmdl_c`
/// into a standalone addon VPK at `vpk_entry` (defaulting to `entry`, so it
/// overrides the base pak). This is a draw-call-only edit (Tier 1a): the shared
/// vertex/index buffers are untouched, so the targeted part simply stops
/// rendering.
///
/// Errors if `material` matches no draw call (so a typo fails loudly rather than
/// repacking the model unchanged); use [`model_draw_call_targets`] to discover
/// the exact names.
pub fn remove_model_material(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    material: &str,
    out_vpk: impl AsRef<Path>,
    vpk_entry: Option<&str>,
) -> Result<MaterialRemovalReport> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;

    let (edited, removed) = morphic::model::remove_draw_calls_by_material(&bytes, material)
        .with_context(|| {
            format!("removing draw calls matching {material:?} from {entry} (try --list-drawcalls)")
        })?;

    let vpk_entry = vpk_entry.unwrap_or(entry).to_string();
    crate::pack(&[(vpk_entry.as_str(), edited.as_slice())], out_vpk.as_ref())
        .with_context(|| format!("packing edited model into {}", out_vpk.as_ref().display()))?;

    Ok(MaterialRemovalReport {
        entry: entry.to_string(),
        vpk_entry,
        removed,
    })
}

/// What a [`replace_model_part`] run swapped and where it was packed.
#[derive(Debug, Clone)]
pub struct PartReplacementReport {
    pub entry: String,
    pub vpk_entry: String,
    pub replaced: ReplacedMeshPart,
}

/// Replaces an existing mesh part's geometry (Tier 1d): reads a new mesh from a
/// `.glb` (one primitive; `glb_mesh` picks it out of a multi-mesh export) and
/// splices it over the model's `mesh_name` part in place, then packs the edited
/// `.vmdl_c` into a standalone addon VPK at `vpk_entry` (default `entry`, so it
/// overrides the base pak).
///
/// The new mesh may have any vertex/index count; it must be skinned against the
/// model skeleton (glTF joint indices == model bone indices) and bound to bones the
/// target part already uses (its bone palette is reused, not grown). The target
/// part must have exactly one vertex buffer, one index buffer, and one draw call
/// (e.g. the gun); errors loudly otherwise. See `docs/handoff-model-edit.md` (T1d).
#[allow(clippy::too_many_arguments)] // VPK in, mesh selectors, addon VPK out
pub fn replace_model_part(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    mesh_name: &str,
    glb_bytes: &[u8],
    glb_mesh: Option<&str>,
    out_vpk: impl AsRef<Path>,
    vpk_entry: Option<&str>,
) -> Result<PartReplacementReport> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;

    let (vb, indices) = morphic::model::read_edited_mesh(glb_bytes, glb_mesh)
        .with_context(|| "reading the replacement mesh from the glb".to_string())?;
    let (edited, replaced) = morphic::model::replace_mesh_part(&bytes, mesh_name, &vb, &indices)
        .with_context(|| format!("replacing mesh part {mesh_name:?} in {entry}"))?;

    let vpk_entry = vpk_entry.unwrap_or(entry).to_string();
    crate::pack(&[(vpk_entry.as_str(), edited.as_slice())], out_vpk.as_ref())
        .with_context(|| format!("packing edited model into {}", out_vpk.as_ref().display()))?;

    Ok(PartReplacementReport {
        entry: entry.to_string(),
        vpk_entry,
        replaced,
    })
}

/// Diagnostic: re-encodes every `MDAT` block of the model unchanged and packs the
/// result into a standalone addon VPK. Used to probe whether the engine accepts our
/// re-encoded model KV3 blocks at all, independent of any edit (see
/// [`morphic::model::reencode_all_mdat_identity`]). Returns the number of blocks
/// re-encoded.
pub fn reencode_model_mdat(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    out_vpk: impl AsRef<Path>,
    vpk_entry: Option<&str>,
) -> Result<usize> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes =
        read_entry(&vpks, entry).with_context(|| format!("model entry {entry} not found"))?;
    let (reencoded, count) = morphic::model::reencode_all_mdat_identity(&bytes)
        .with_context(|| format!("re-encoding MDAT blocks of {entry}"))?;
    let vpk_entry = vpk_entry.unwrap_or(entry);
    crate::pack(&[(vpk_entry, reencoded.as_slice())], out_vpk.as_ref()).with_context(|| {
        format!(
            "packing re-encoded model into {}",
            out_vpk.as_ref().display()
        )
    })?;
    Ok(count)
}

/// Mean position over all the given buffers (the shared scale pivot).
// The vertex count -> f32 cast is an averaging divisor; precision loss at
// realistic mesh sizes does not meaningfully move the centroid.
#[allow(clippy::cast_precision_loss)]
fn shared_centroid<'a>(buffers: impl Iterator<Item = &'a [[f32; 3]]>) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut count = 0usize;
    for positions in buffers {
        for p in positions {
            for k in 0..3 {
                sum[k] += p[k];
            }
        }
        count += positions.len();
    }
    if count == 0 {
        return [0.0; 3];
    }
    let n = count as f32;
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

/// Scale each position about `centroid`, then translate.
fn transform_about(
    positions: &[[f32; 3]],
    centroid: [f32; 3],
    edit: &GeometryEdit,
) -> Vec<[f32; 3]> {
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

/// Resolves and bakes a WIP hero's loose NM menu/idle pose. The hero's `.vmdl_c`
/// embeds no clips; its pose lives in `<model_dir>/clips/<name>.vnmclip_c` with a
/// sibling `<h>.vnmskel_c`. For each candidate clip name this tries the bare name
/// and a `<stem>_<name>` variant (Mina prefixes the codename, e.g.
/// `vampirebat_hero_pose`), reads the `.vnmskel_c` the clip references, and bakes
/// the static pose onto `model`. `None` when no loose pose clip resolves.
fn bake_loose_nm_pose(
    vpks: &[valve_pak::VPK],
    entry: &str,
    model: &morphic::model::Model,
    candidates: &[&str],
) -> Option<morphic::model::Model> {
    let (dir, file) = entry.rsplit_once('/')?;
    let stem = file.strip_suffix(".vmdl_c").unwrap_or(file);
    for cand in candidates {
        for name in [(*cand).to_string(), format!("{stem}_{cand}")] {
            let clip_path = format!("{dir}/clips/{name}.vnmclip_c");
            let Some(clip_bytes) = read_entry(vpks, &clip_path) else {
                continue;
            };
            let Ok(pose) = morphic::model::decode_nm_pose(&clip_bytes) else {
                continue;
            };
            let skel_path = nm_skeleton_entry(&pose.skeleton_ref, dir, stem);
            let Some(skel_bytes) = read_entry(vpks, &skel_path) else {
                continue;
            };
            let Ok(skel) = morphic::model::decode_nm_skeleton(&skel_bytes) else {
                continue;
            };
            if let Ok(posed) = morphic::model::bake_nm_pose(model, &skel, &pose) {
                return Some(posed);
            }
        }
    }
    None
}

/// Compiled `.vnmskel_c` entry path for a clip's `m_skeleton` reference (append
/// `_c` to the uncompiled path), falling back to the conventional
/// `<dir>/<stem>.vnmskel_c` when the reference is absent.
fn nm_skeleton_entry(skeleton_ref: &str, dir: &str, stem: &str) -> String {
    if skeleton_ref.is_empty() {
        format!("{dir}/{stem}.vnmskel_c")
    } else if skeleton_ref.ends_with("_c") {
        skeleton_ref.to_string()
    } else {
        format!("{skeleton_ref}_c")
    }
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

#[cfg(test)]
mod tests {
    // The transform arithmetic here is exact (scale 2.0, integer inputs), so the
    // tests assert exact float-array equality deliberately.
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn shared_centroid_is_the_mean_across_buffers() {
        let a = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let b = [[0.0, 4.0, 0.0], [2.0, 4.0, 0.0]];
        let c = shared_centroid([a.as_slice(), b.as_slice()].into_iter());
        assert_eq!(c, [1.0, 2.0, 0.0]);
    }

    /// Two buffers scaled about one shared centroid stay rigid relative to each
    /// other (the bug a per-buffer centroid would introduce): a point and the
    /// centroid keep the same scaled offset regardless of which buffer it is in.
    #[test]
    fn scale_about_shared_centroid_keeps_buffers_aligned() {
        let centroid = [1.0, 2.0, 0.0];
        let edit = GeometryEdit {
            part: None,
            scale: 2.0,
            translate: [0.0; 3],
        };
        // A vertex at the centroid stays put under a pure scale.
        let at_centroid = transform_about(&[centroid], centroid, &edit);
        assert_eq!(at_centroid[0], centroid);
        // A vertex 1 unit from the centroid moves to 2 units (scale 2x), same
        // rule in any buffer.
        let off = transform_about(&[[2.0, 2.0, 0.0]], centroid, &edit);
        assert_eq!(off[0], [3.0, 2.0, 0.0]);
    }

    #[test]
    fn translate_is_uniform_and_centroid_independent() {
        let edit = GeometryEdit {
            part: None,
            scale: 1.0,
            translate: [10.0, -5.0, 1.0],
        };
        let out = transform_about(
            &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
            [99.0, 99.0, 99.0],
            &edit,
        );
        assert_eq!(out, vec![[10.0, -5.0, 1.0], [11.0, -4.0, 2.0]]);
    }
}
