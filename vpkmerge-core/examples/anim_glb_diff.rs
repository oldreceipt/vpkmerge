//! Diagnostic: compare a `.glb` animation against a slot clip after the importer's
//! name-mapping + resample. Reports the per-bone rotation angle difference (mean
//! and worst offenders) between the original clip and what `apply_animation`
//! produces from the glb. Used to check whether a Blender import/export round-trip
//! preserved the per-bone coordinate frame: a preserved frame reads as a small
//! difference (resampling noise); a broken frame reads as tens of degrees.
//!
//! Usage:
//!   cargo run --release -p vpkmerge-core --example anim_glb_diff -- \
//!       <pak01_dir.vpk> <clip_entry.vnmclip_c> <anim.glb>

#![allow(
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use anyhow::{Context, Result};
use morphic::model::{
    apply_animation, decode_nm_clip, decode_nm_skeleton, read_glb_animation, Quat,
};

fn angle_between(a: Quat, b: Quat) -> f32 {
    let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w)
        .abs()
        .clamp(0.0, 1.0);
    2.0 * dot.acos()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: pak01_dir.vpk")?;
    let clip_entry = args.next().context("missing arg: clip entry")?;
    let glb_path = args.next().context("missing arg: anim.glb")?;

    let clip_bytes = vpkmerge_core::read_vpk_entry(&pak, &clip_entry)?;
    let clip = decode_nm_clip(&clip_bytes).context("decode clip")?;
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(
        &pak,
        &resolve_skel(&clip.skeleton_ref),
    )?)
    .context("decode vnmskel")?;
    let glb = std::fs::read(&glb_path).with_context(|| format!("reading {glb_path}"))?;
    let anim = read_glb_animation(&glb, None).context("read glb animation")?;
    let edited = apply_animation(&clip, &skel, &anim);

    println!(
        "clip {clip_entry}: {} frames, {} tracks; glb animates {} bone(s)",
        clip.frame_count,
        clip.tracks.len(),
        anim.bones.len()
    );

    // Compare rotation tracks present in BOTH the original clip and the import.
    let mut per_bone: Vec<(String, f32, f32)> = Vec::new(); // (bone, mean_deg, max_deg)
    let mut global_sum = 0.0f64;
    let mut global_n = 0usize;
    for (i, (a, b)) in clip.tracks.iter().zip(edited.tracks.iter()).enumerate() {
        let (Some(ra), Some(rb)) = (&a.rotations, &b.rotations) else {
            continue;
        };
        // Only bones the glb actually animated (others are unchanged copies).
        let name = skel.bone_names.get(i).cloned().unwrap_or_default();
        if anim.bones.get(&name).is_none_or(|t| t.rotations.is_none()) {
            continue;
        }
        let n = ra.len().min(rb.len());
        if n == 0 {
            continue;
        }
        let mut sum = 0.0f32;
        let mut mx = 0.0f32;
        for (qa, qb) in ra.iter().zip(rb.iter()).take(n) {
            let ang = angle_between(*qa, *qb).to_degrees();
            sum += ang;
            mx = mx.max(ang);
        }
        let mean = sum / n as f32;
        global_sum += f64::from(sum);
        global_n += n;
        per_bone.push((name, mean, mx));
    }

    anyhow::ensure!(
        !per_bone.is_empty(),
        "no rotation bone overlaps between the clip and the glb"
    );

    let overall_mean = (global_sum / global_n as f64) as f32;
    per_bone.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    println!(
        "compared {} bone rotation track(s); overall mean diff {overall_mean:.2} deg",
        per_bone.len()
    );
    println!("worst offenders (bone: mean / max deg):");
    for (name, mean, mx) in per_bone.iter().take(10) {
        println!("  {name:<20} {mean:6.2} / {mx:6.2}");
    }

    let verdict = if overall_mean < 5.0 {
        "FRAME PRESERVED (small diff = resampling noise; Blender authoring is safe)"
    } else if overall_mean < 20.0 {
        "MARGINAL (some drift; check the worst offenders)"
    } else {
        "FRAME BROKEN (large diff; a coordinate correction is needed)"
    };
    println!("verdict: {verdict}");
    Ok(())
}

fn resolve_skel(reference: &str) -> String {
    if reference.ends_with("_c") {
        reference.to_owned()
    } else {
        format!("{reference}_c")
    }
}
