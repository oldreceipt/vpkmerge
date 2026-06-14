//! Report frame count, duration, additive flag, and whether both arms are
//! animated for a set of `.vnmclip_c` entries. Used to pick a slot to author a
//! custom animation into: a continuous deadpan loop wants a LONG, non-additive
//! clip that drives the arm bones (so an authored arm motion is not bone-masked
//! out, and one seamless cycle per loop is slow enough to read).
//!
//! Usage:
//!   cargo run --release -p vpkmerge-core --example clip_info -- \
//!       <pak01_dir.vpk> <entry.vnmclip_c> [more entries...]

#![allow(clippy::doc_markdown)]

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: pak01_dir.vpk")?;
    let entries: Vec<String> = args.collect();
    anyhow::ensure!(!entries.is_empty(), "give one or more clip entries");

    // Resolve the skeleton once from the first decodable clip (all share a rig).
    let mut rows: Vec<(String, u32, f32, f32, bool, bool, bool)> = Vec::new();
    let mut skel: Option<morphic::model::NmSkeleton> = None;
    for entry in &entries {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            eprintln!("  (skip, not in vpk) {entry}");
            continue;
        };
        let clip =
            morphic::model::decode_nm_clip(&bytes).with_context(|| format!("decode {entry}"))?;
        if skel.is_none() {
            let skel_entry = resolve_skel(&clip.skeleton_ref);
            if let Ok(sb) = vpkmerge_core::read_vpk_entry(&pak, &skel_entry) {
                skel = morphic::model::decode_nm_skeleton(&sb).ok();
            }
        }
        // Is a bone animated in rotation? (proxy for the slot driving it.)
        let animated = |name: &str| -> bool {
            let Some(s) = &skel else { return false };
            s.bone_names
                .iter()
                .position(|b| b == name)
                .and_then(|i| clip.tracks.get(i))
                .is_some_and(|t| t.rotations.is_some())
        };
        let arm_l = animated("arm_upper_L");
        let arm_r = animated("arm_upper_R");
        let short = entry.rsplit('/').next().unwrap_or(entry).to_owned();
        rows.push((
            short,
            clip.frame_count,
            clip.fps(),
            clip.duration,
            clip.additive,
            arm_l,
            arm_r,
        ));
    }

    rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    println!(
        "{:<34} {:>6} {:>6} {:>7}  {:<8} {:<6}",
        "clip", "frames", "fps", "dur(s)", "additive", "arms"
    );
    for (name, frames, fps, dur, additive, arm_l, arm_r) in &rows {
        let arms = match (arm_l, arm_r) {
            (true, true) => "L+R",
            (true, false) => "L",
            (false, true) => "R",
            (false, false) => "-",
        };
        println!(
            "{name:<34} {frames:>6} {fps:>6.1} {dur:>7.3}  {:<8} {arms:<6}",
            if *additive { "ADD" } else { "abs" }
        );
    }
    Ok(())
}

fn resolve_skel(reference: &str) -> String {
    if reference.ends_with("_c") {
        reference.to_owned()
    } else {
        format!("{reference}_c")
    }
}
