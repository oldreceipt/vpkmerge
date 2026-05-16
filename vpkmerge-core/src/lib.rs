//! Core VPK merging logic. Pure Rust, no UI or framework dependencies.
//!
//! Two entry points:
//!   - [`inspect`] opens a single VPK and returns its path list and metadata
//!   - [`merge`] combines several VPKs into one, with last-input-wins on collision

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct ModInfo {
    pub path: PathBuf,
    pub name: String,
    pub file_count: usize,
    pub file_paths: Vec<String>,
}

pub struct MergeReport {
    pub total_entries: usize,
    pub overridden: usize,
    pub inputs: usize,
    pub output_path: PathBuf,
}

/// Open a single VPK and return its file list plus a display name.
pub fn inspect<P: AsRef<Path>>(path: P) -> Result<ModInfo> {
    let path = path.as_ref();
    let vpk = valve_pak::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let file_paths: Vec<String> = vpk.file_paths().cloned().collect();
    Ok(ModInfo {
        path: path.to_path_buf(),
        name,
        file_count: file_paths.len(),
        file_paths,
    })
}

/// Combine `ordered_inputs` into a single VPK at `output`. Later inputs in the
/// slice win on path collision.
pub fn merge<P: AsRef<Path>, O: AsRef<Path>>(
    ordered_inputs: &[P],
    output: O,
) -> Result<MergeReport> {
    if ordered_inputs.len() < 2 {
        anyhow::bail!("need at least 2 input VPKs to merge");
    }
    let output = output.as_ref().to_path_buf();

    let vpks: Vec<_> = ordered_inputs
        .iter()
        .map(|p| {
            let p = p.as_ref();
            valve_pak::open(p).with_context(|| format!("opening {}", p.display()))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut winner: HashMap<String, usize> = HashMap::new();
    let mut collisions = 0usize;
    for (idx, vpk) in vpks.iter().enumerate() {
        for p in vpk.file_paths() {
            if winner.insert(p.clone(), idx).is_some() {
                collisions += 1;
            }
        }
    }

    let tmp = tempfile::tempdir().context("creating temp directory")?;
    for (path, idx) in &winner {
        let mut vf = vpks[*idx]
            .get_file(path)
            .with_context(|| format!("locating {} in input {}", path, idx))?;
        let bytes = vf
            .read_all()
            .with_context(|| format!("reading {}", path))?;
        let dst = tmp.path().join(path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        std::fs::write(&dst, &bytes)
            .with_context(|| format!("writing {}", dst.display()))?;
    }

    let merged = valve_pak::from_directory(tmp.path()).context("packing merged VPK")?;
    merged
        .save(&output)
        .with_context(|| format!("saving {}", output.display()))?;

    Ok(MergeReport {
        total_entries: winner.len(),
        overridden: collisions,
        inputs: ordered_inputs.len(),
        output_path: output,
    })
}
