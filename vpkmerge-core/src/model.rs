//! Model (`.vmdl_c`) orchestration: open a VPK, find compiled models, and hand
//! their bytes to `morphic` for decode. Mirrors [`crate::portrait`]. Today it
//! exposes [`inspect_models`] (a structural read); glTF export lands later
//! (see `docs/vmdl-glb-exporter.md`).

use anyhow::{Context, Result};
use std::path::Path;

pub use morphic::model::{BlockSummary, ModelInfo};

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
        let has_own_clip = candidates
            .iter()
            .any(|c| model.animations.iter().any(|a| a.name.eq_ignore_ascii_case(c)));
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
