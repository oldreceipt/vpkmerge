//! Model (`.vmdl_c`) orchestration: open a VPK, find compiled models, and hand
//! their bytes to `morphic` for decode. Mirrors [`crate::portrait`]. Today it
//! exposes [`inspect_models`] (a structural read); glTF export lands later
//! (see `docs/vmdl-glb-exporter.md`).

use anyhow::{Context, Result};
use std::path::Path;

pub use morphic::model::{BlockSummary, ModelInfo};

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

/// Decode a `.vmdl_c` from a VPK and write it as a textured binary glTF.
///
/// `vpk` is where the model entry lives (a skin VPK or the base pak); `base`, if
/// given, is the base `pak01_dir.vpk` that materials/textures resolve against
/// when the skin does not ship them. The mesh + skeleton + skin come from the
/// model entry; materials are textured via the cross-VPK resolver.
pub fn export_model(
    vpk: impl AsRef<Path>,
    entry: &str,
    base: Option<&Path>,
    out: impl AsRef<Path>,
) -> Result<()> {
    let vpk_path = vpk.as_ref();
    let out = out.as_ref();

    let skin =
        valve_pak::open(vpk_path).with_context(|| format!("opening {}", vpk_path.display()))?;
    let mut vf = skin
        .get_file(entry)
        .with_context(|| format!("locating {entry} in {}", vpk_path.display()))?;
    let bytes = vf.read_all().with_context(|| format!("reading {entry}"))?;

    let model = morphic::model::decode(&bytes).with_context(|| format!("decoding {entry}"))?;

    let mut vpks = vec![skin];
    if let Some(base) = base {
        vpks.push(valve_pak::open(base).with_context(|| format!("opening {}", base.display()))?);
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
