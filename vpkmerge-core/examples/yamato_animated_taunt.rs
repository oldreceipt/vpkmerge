//! Author the first hand-made Deadlock *motion* (not just a static pose). Decodes
//! Yamato's animated `reload_idle_quick` clip with the new pose codec, layers an
//! authored "bow" onto the spine/neck/head **rotation tracks across all frames**
//! (a tilt that ramps in then out over the clip), re-encodes the quantized pose
//! stream, and splices it back **byte-faithfully** (the edited stream is the same
//! length as the original, since no channel is added or removed). Packs the result
//! at `reload_idle` + `reload_idle_quick` so pressing R plays the custom motion.
//!
//! Inspect (list animated-rotation bones):
//!   cargo run --release -p vpkmerge-core --example yamato_animated_taunt -- <pak01_dir.vpk>
//! Build the addon (+ frame-0 and apex GLBs to eyeball):
//!   cargo run --release -p vpkmerge-core --example yamato_animated_taunt -- <pak01_dir.vpk> <out_dir>

use std::collections::HashMap;
use std::f32::consts::PI;

use anyhow::{Context, Result};
use morphic::model::{
    bake_nm_pose, decode, decode_nm_clip, decode_nm_skeleton, to_glb, LocalPose, NmClip, NmPose,
    Quat, Vec3,
};

const CLIP: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const RELOAD_IDLE: &str = "models/heroes_wip/yamato/clips/reload_idle.vnmclip_c";
const RELOAD_IDLE_QUICK: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

/// Bones whose animated rotation tracks the bow is layered onto (the closer to
/// the head, the more the tilt accumulates down the chain).
const BOW_BONES: &[&str] = &["spine_0", "spine_1", "spine_2", "spine_3", "neck_0", "head"];
const BOW_DEGREES: f32 = 22.0; // per-bone peak tilt; accumulates down the spine

fn qmul(a: Quat, b: Quat) -> Quat {
    Quat {
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    }
}

fn normalize(q: Quat) -> Quat {
    let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    Quat {
        x: q.x / n,
        y: q.y / n,
        z: q.z / n,
        w: q.w / n,
    }
}

fn axis_angle(axis: [f32; 3], deg: f32) -> Quat {
    let r = deg.to_radians() * 0.5;
    let s = r.sin();
    Quat {
        x: axis[0] * s,
        y: axis[1] * s,
        z: axis[2] * s,
        w: r.cos(),
    }
}

/// Builds an `NmPose` (one `LocalPose` per bone) for a single frame of a clip, so
/// the existing static-pose baker can render that frame for a visual check.
fn pose_at(clip: &NmClip, frame: usize) -> NmPose {
    let bones = clip
        .tracks
        .iter()
        .map(|t| {
            let s = &t.settings;
            let rotation = t
                .rotations
                .as_ref()
                .and_then(|r| r.get(frame).copied())
                .unwrap_or(s.constant_rotation);
            let translation = t
                .translations
                .as_ref()
                .and_then(|v| v.get(frame).copied())
                .unwrap_or(Vec3 {
                    x: s.translation_range[0].start,
                    y: s.translation_range[1].start,
                    z: s.translation_range[2].start,
                });
            let scale = t
                .scales
                .as_ref()
                .and_then(|v| v.get(frame).copied())
                .unwrap_or(s.scale_range.start);
            Some(LocalPose {
                translation,
                rotation,
                scale,
            })
        })
        .collect();
    NmPose {
        skeleton_ref: clip.skeleton_ref.clone(),
        frame_count: clip.frame_count,
        bones,
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next();

    let clip_bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let clip = decode_nm_clip(&clip_bytes)?;
    let frames = clip.frame_count as usize;
    println!(
        "clip {CLIP}\n  {} frames | {} bones | {} byte pose stream",
        frames,
        clip.tracks.len(),
        clip.compressed_pose_data.len()
    );

    // name -> bone index, and which bones have an animated rotation track.
    let idx: HashMap<&str, usize> = skel
        .bone_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    if out_dir.is_none() {
        println!("\nbones with an animated rotation track:");
        for (i, t) in clip.tracks.iter().enumerate() {
            if t.rotations.is_some() {
                let name = skel.bone_names.get(i).map_or("?", String::as_str);
                println!("  {i:>3} {name}");
            }
        }
        println!("\n(inspect mode; pass an out-dir to build the addon)");
        return Ok(());
    }
    let out_dir = out_dir.unwrap();

    // Layer the bow onto each target bone's animated rotation track. The tilt
    // ramps 0 -> peak -> 0 over the clip (sin over [0, pi]); bones further down
    // the chain add a touch more so the whole upper body bends.
    let mut edited = clip.clone();
    let mut touched = Vec::new();
    for (depth, name) in BOW_BONES.iter().enumerate() {
        let Some(&b) = idx.get(name) else { continue };
        let Some(track) = edited.tracks.get_mut(b) else {
            continue;
        };
        let Some(rots) = track.rotations.as_mut() else {
            println!("  {name}: rotation not animated in this clip; skipping");
            continue;
        };
        #[allow(clippy::cast_precision_loss)]
        let peak = BOW_DEGREES * (1.0 + depth as f32 * 0.15);
        for (f, q) in rots.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let t = if frames > 1 {
                f as f32 / (frames - 1) as f32
            } else {
                0.0
            };
            let amount = peak * (PI * t).sin(); // 0 at the ends, peak mid-clip
            *q = normalize(qmul(*q, axis_angle([1.0, 0.0, 0.0], amount)));
        }
        touched.push(*name);
    }
    println!("bowed {} bones: {touched:?}", touched.len());
    anyhow::ensure!(
        !touched.is_empty(),
        "no target bone had an animated rotation"
    );

    // Re-encode the pose stream. It must be the same length (same channels), which
    // is the precondition for the byte-faithful in-place splice.
    let (new_blob, new_offsets) = morphic::model::encode_compressed_pose(&edited);
    anyhow::ensure!(
        new_blob.len() == clip.compressed_pose_data.len(),
        "re-encoded stream length changed ({} -> {}); cannot splice in place",
        clip.compressed_pose_data.len(),
        new_blob.len()
    );
    anyhow::ensure!(
        new_offsets == clip.compressed_pose_offsets,
        "frame offsets changed"
    );

    // Splice the new stream into the resource, byte-faithfully.
    let patched =
        morphic::patch_kv3_resource_blob(&clip_bytes, &clip.compressed_pose_data, &new_blob)
            .context("splicing edited pose stream")?;

    // Verify: re-decode the patched clip and confirm the bow is present and only
    // the targeted bones moved.
    let redec = decode_nm_clip(&patched).context("re-decode patched clip")?;
    let mut max_added = 0f32;
    for (i, (a, b)) in clip.tracks.iter().zip(redec.tracks.iter()).enumerate() {
        if let (Some(ra), Some(rb)) = (&a.rotations, &b.rotations) {
            for (qa, qb) in ra.iter().zip(rb.iter()) {
                let dot = (qa.x * qb.x + qa.y * qb.y + qa.z * qb.z + qa.w * qb.w)
                    .abs()
                    .clamp(0.0, 1.0);
                let ang = 2.0 * dot.acos();
                let is_target = BOW_BONES.iter().any(|n| idx.get(n) == Some(&i));
                if !is_target {
                    anyhow::ensure!(ang < 0.01, "non-target bone {i} moved {ang} rad");
                }
                max_added = max_added.max(ang);
            }
        }
    }
    println!(
        "re-decode OK; max added rotation {:.1} deg on targeted bones",
        max_added.to_degrees()
    );

    // Eyeball: bake frame 0 (start) and the apex frame onto Yamato's mesh.
    let model = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
    let apex = frames / 2;
    for (label, f) in [("start", 0), ("apex", apex)] {
        let baked = bake_nm_pose(&model, &skel, &pose_at(&redec, f))?;
        let glb = to_glb(&baked)?;
        let path = format!("{out_dir}/yamato_bow_{label}_frame{f}.glb");
        std::fs::write(&path, &glb)?;
        println!("wrote {path} ({} bytes)", glb.len());
    }

    // Pack at both reload slots -> one addon VPK.
    let out_vpk = format!("{out_dir}/yamato_reload_bow_dir.vpk");
    vpkmerge_core::pack(
        &[
            (RELOAD_IDLE, patched.as_slice()),
            (RELOAD_IDLE_QUICK, patched.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked custom MOTION at reload_idle + reload_idle_quick -> {out_vpk}");
    println!("install: copy to game/citadel/addons/ as a free pakNN_dir.vpk; press R in-game");
    Ok(())
}
