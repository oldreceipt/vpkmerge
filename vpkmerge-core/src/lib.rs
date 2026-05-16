//! Core VPK merging logic. Pure Rust, no UI or framework dependencies.
//!
//! ```no_run
//! use vpkmerge_core::{merge, MergeOptions};
//!
//! let report = merge(
//!     &["mod_a_dir.vpk", "mod_b_dir.vpk"],
//!     "combined_dir.vpk",
//!     &MergeOptions::default(),
//! ).unwrap();
//! println!("Wrote {} entries", report.total_entries);
//! ```

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModInfo {
    pub path: PathBuf,
    pub name: String,
    pub file_count: usize,
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub path: String,
    /// Indices into the input slice, in input order.
    pub owner_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Later inputs override earlier ones (default).
    LastWins,
    /// Earlier inputs win; later duplicates are dropped.
    FirstWins,
    /// Refuse to merge if any path appears in more than one input.
    Error,
}

#[derive(Clone, Debug)]
pub struct MergeOptions {
    pub collision_policy: CollisionPolicy,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            collision_policy: CollisionPolicy::LastWins,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MergeReport {
    pub total_entries: usize,
    /// Number of distinct paths whose content was overridden by a later input.
    pub overridden_paths: usize,
    pub inputs: usize,
    pub output_path: PathBuf,
}

/// Open a single VPK and return its file list plus a display name.
pub fn inspect<P: AsRef<Path>>(path: P) -> Result<ModInfo> {
    let path = path.as_ref();
    let vpk = valve_pak::open(path).with_context(|| format!("opening {}", path.display()))?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let file_paths: Vec<String> = vpk.file_paths().cloned().collect();
    Ok(ModInfo {
        path: path.to_path_buf(),
        name,
        file_count: file_paths.len(),
        file_paths,
    })
}

/// Compute the set of paths that appear in more than one input, without
/// performing the merge. Cheaper than a full merge and useful for previewing.
pub fn detect_conflicts<P: AsRef<Path>>(inputs: &[P]) -> Result<Vec<Conflict>> {
    let vpks = open_all(inputs)?;
    let owners = compute_owners(&vpks);
    let mut conflicts: Vec<Conflict> = owners
        .into_iter()
        .filter(|(_, idxs)| idxs.len() > 1)
        .map(|(path, owner_indices)| Conflict {
            path,
            owner_indices,
        })
        .collect();
    conflicts.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(conflicts)
}

/// Combine `ordered_inputs` into a single VPK at `output`, applying
/// `options.collision_policy` when the same path appears in more than one
/// input.
pub fn merge<P: AsRef<Path>, O: AsRef<Path>>(
    ordered_inputs: &[P],
    output: O,
    options: &MergeOptions,
) -> Result<MergeReport> {
    if ordered_inputs.len() < 2 {
        anyhow::bail!("need at least 2 input VPKs to merge");
    }
    let output = output.as_ref().to_path_buf();

    reject_output_equals_input(ordered_inputs, &output)?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
        }
    }

    let vpks = open_all(ordered_inputs)?;
    let owners = compute_owners(&vpks);
    let winners = resolve_winners(&owners, options.collision_policy)?;
    let overridden_paths = owners.values().filter(|v| v.len() > 1).count();

    let tmp = tempfile::tempdir().context("creating temp directory")?;
    for (path, idx) in &winners {
        let mut vf = vpks[*idx]
            .get_file(path)
            .with_context(|| format!("locating {path} in input {idx}"))?;
        let bytes = vf.read_all().with_context(|| format!("reading {path}"))?;
        let dst = tmp.path().join(path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        std::fs::write(&dst, &bytes).with_context(|| format!("writing {}", dst.display()))?;
    }

    let merged = valve_pak::from_directory(tmp.path()).context("packing merged VPK")?;
    merged
        .save(&output)
        .with_context(|| format!("saving {}", output.display()))?;

    Ok(MergeReport {
        total_entries: winners.len(),
        overridden_paths,
        inputs: ordered_inputs.len(),
        output_path: output,
    })
}

fn open_all<P: AsRef<Path>>(inputs: &[P]) -> Result<Vec<valve_pak::VPK>> {
    inputs
        .iter()
        .map(|p| {
            let p = p.as_ref();
            valve_pak::open(p).with_context(|| format!("opening {}", p.display()))
        })
        .collect()
}

fn compute_owners(vpks: &[valve_pak::VPK]) -> BTreeMap<String, Vec<usize>> {
    let mut owners: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, vpk) in vpks.iter().enumerate() {
        for p in vpk.file_paths() {
            owners.entry(p.clone()).or_default().push(idx);
        }
    }
    owners
}

fn resolve_winners(
    owners: &BTreeMap<String, Vec<usize>>,
    policy: CollisionPolicy,
) -> Result<BTreeMap<String, usize>> {
    if policy == CollisionPolicy::Error {
        let conflicts: Vec<&str> = owners
            .iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(k, _)| k.as_str())
            .collect();
        if !conflicts.is_empty() {
            let sample = conflicts
                .iter()
                .take(5)
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "{} path conflict{} (collision policy is Error). First: {}",
                conflicts.len(),
                if conflicts.len() == 1 { "" } else { "s" },
                sample
            );
        }
    }

    Ok(owners
        .iter()
        .map(|(path, idxs)| {
            let winner = match policy {
                CollisionPolicy::FirstWins | CollisionPolicy::Error => *idxs.first().unwrap(),
                CollisionPolicy::LastWins => *idxs.last().unwrap(),
            };
            (path.clone(), winner)
        })
        .collect())
}

fn reject_output_equals_input<P: AsRef<Path>>(inputs: &[P], output: &Path) -> Result<()> {
    let canonical_output = output.parent().and_then(|p| {
        if p.as_os_str().is_empty() {
            None
        } else {
            p.canonicalize()
                .ok()
                .and_then(|p| output.file_name().map(|f| p.join(f)))
        }
    });
    let Some(canonical_output) = canonical_output else {
        return Ok(());
    };
    let mut seen = HashSet::new();
    for input in inputs {
        if let Ok(canon) = input.as_ref().canonicalize() {
            if canon == canonical_output {
                anyhow::bail!("output path equals input path: {}", canon.display());
            }
            if !seen.insert(canon.clone()) {
                anyhow::bail!("input listed twice: {}", canon.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};

    fn make_vpk(path: &Path, files: &[(&str, &[u8])]) -> Result<()> {
        let src = tempdir()?;
        for (rel, bytes) in files {
            let p = src.path().join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, bytes)?;
        }
        let vpk = valve_pak::from_directory(src.path())?;
        vpk.save(path)?;
        Ok(())
    }

    fn read_entry(vpk_path: &Path, entry: &str) -> Result<Vec<u8>> {
        let vpk = valve_pak::open(vpk_path)?;
        let mut f = vpk.get_file(entry)?;
        f.read_all()
    }

    struct Fixture {
        dir: TempDir,
        a: PathBuf,
        b: PathBuf,
        out: PathBuf,
    }

    fn two_inputs() -> Result<Fixture> {
        let tmp = tempdir()?;
        let a = tmp.path().join("a_dir.vpk");
        let b = tmp.path().join("b_dir.vpk");
        let out = tmp.path().join("merged_dir.vpk");
        make_vpk(
            &a,
            &[
                ("only_a/file.txt", b"a-only"),
                ("shared/file.txt", b"from a"),
            ],
        )?;
        make_vpk(
            &b,
            &[
                ("only_b/file.txt", b"b-only"),
                ("shared/file.txt", b"from b"),
            ],
        )?;
        Ok(Fixture {
            dir: tmp,
            a,
            b,
            out,
        })
    }

    #[test]
    fn last_wins() -> Result<()> {
        let fx = two_inputs()?;
        let report = merge(&[&fx.a, &fx.b], &fx.out, &MergeOptions::default())?;
        assert_eq!(report.total_entries, 3);
        assert_eq!(report.overridden_paths, 1);
        assert_eq!(report.inputs, 2);
        assert_eq!(read_entry(&fx.out, "shared/file.txt")?, b"from b");
        assert_eq!(read_entry(&fx.out, "only_a/file.txt")?, b"a-only");
        assert_eq!(read_entry(&fx.out, "only_b/file.txt")?, b"b-only");
        Ok(())
    }

    #[test]
    fn first_wins() -> Result<()> {
        let fx = two_inputs()?;
        let opts = MergeOptions {
            collision_policy: CollisionPolicy::FirstWins,
        };
        merge(&[&fx.a, &fx.b], &fx.out, &opts)?;
        assert_eq!(read_entry(&fx.out, "shared/file.txt")?, b"from a");
        Ok(())
    }

    #[test]
    fn error_policy_rejects_conflicts() -> Result<()> {
        let fx = two_inputs()?;
        let opts = MergeOptions {
            collision_policy: CollisionPolicy::Error,
        };
        let result = merge(&[&fx.a, &fx.b], &fx.out, &opts);
        assert!(result.is_err(), "expected error policy to reject");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("conflict"), "msg = {msg}");
        Ok(())
    }

    #[test]
    fn detect_conflicts_reports_overlap() -> Result<()> {
        let fx = two_inputs()?;
        let conflicts = detect_conflicts(&[&fx.a, &fx.b])?;
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "shared/file.txt");
        assert_eq!(conflicts[0].owner_indices, vec![0, 1]);
        Ok(())
    }

    #[test]
    fn rejects_too_few_inputs() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("out_dir.vpk");
        let err = merge::<&Path, _>(&[], &out, &MergeOptions::default()).unwrap_err();
        assert!(format!("{err:#}").contains("at least 2"));
    }

    #[test]
    fn rejects_output_equals_input() -> Result<()> {
        let fx = two_inputs()?;
        // Try to merge into one of the inputs.
        let err = merge(&[&fx.a, &fx.b], &fx.a, &MergeOptions::default()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("equals input"), "msg = {msg}");
        Ok(())
    }

    #[test]
    fn creates_missing_parent_dir() -> Result<()> {
        let fx = two_inputs()?;
        let nested = fx.dir.path().join("does/not/exist/yet/out_dir.vpk");
        merge(&[&fx.a, &fx.b], &nested, &MergeOptions::default())?;
        assert!(nested.exists());
        Ok(())
    }

    #[test]
    fn stable_entry_set() -> Result<()> {
        // Same inputs merged twice produce the same set of entries.
        // (Byte-exact order isn't guaranteed: valve_pak::from_directory walks
        // the filesystem in OS-dependent order. Set equality is what's
        // semantically required.)
        let fx = two_inputs()?;
        let out1 = fx.dir.path().join("m1_dir.vpk");
        let out2 = fx.dir.path().join("m2_dir.vpk");
        merge(&[&fx.a, &fx.b], &out1, &MergeOptions::default())?;
        merge(&[&fx.a, &fx.b], &out2, &MergeOptions::default())?;
        let mut p1: Vec<_> = valve_pak::open(&out1)?.file_paths().cloned().collect();
        let mut p2: Vec<_> = valve_pak::open(&out2)?.file_paths().cloned().collect();
        p1.sort();
        p2.sort();
        assert_eq!(p1, p2);
        Ok(())
    }
}
