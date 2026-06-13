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
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, to_glb, Model, Quat, Vec3,
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
const BOW_SIGN: f32 = 1.0; // flip to -1.0 if the bow leans backward instead of forward

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

fn axis_angle(axis: Vec3, deg: f32) -> Quat {
    let r = deg.to_radians() * 0.5;
    let s = r.sin();
    Quat {
        x: axis.x * s,
        y: axis.y * s,
        z: axis.z * s,
        w: r.cos(),
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn model_bone(model: &Model, name: &str) -> Option<usize> {
    model.skeleton.bones.iter().position(|b| b.name == name)
}

/// World-space position of a model bone's origin (its bind global translation).
fn world_pos(model: &Model, name: &str) -> Option<Vec3> {
    let i = model_bone(model, name)?;
    Some(
        model.skeleton.bones[i]
            .global_bind
            .transform_point(Vec3::default()),
    )
}

/// Derive the "bow" rotation axis in world space from the skeleton's own
/// geometry. Up is pelvis -> head; forward is the feet's facing (ankle -> ball,
/// flattened to horizontal); the bow axis is `up x forward`, so a *positive*
/// rotation swings the head from up toward forward (a true forward bow). Pinning
/// forward to the feet removes the sign guess and the sideways twist the earlier
/// hardcoded local axis produced.
fn bow_axis_world(model: &Model) -> Option<Vec3> {
    let up = sub(world_pos(model, "head")?, world_pos(model, "pelvis")?).normalized();
    let toe_l = sub(world_pos(model, "ball_L")?, world_pos(model, "ankle_L")?);
    let toe_r = sub(world_pos(model, "ball_R")?, world_pos(model, "ankle_R")?);
    let toe = Vec3 {
        x: toe_l.x + toe_r.x,
        y: toe_l.y + toe_r.y,
        z: toe_l.z + toe_r.z,
    };
    // Flatten the feet-forward vector into the plane perpendicular to up.
    let d = toe.x * up.x + toe.y * up.y + toe.z * up.z;
    let forward = Vec3 {
        x: toe.x - d * up.x,
        y: toe.y - d * up.y,
        z: toe.z - d * up.z,
    }
    .normalized();
    Some(cross(up, forward).normalized())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next();

    let clip_bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let clip = decode_nm_clip(&clip_bytes)?;
    let model = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
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

    // The bow axis in world space, from the skeleton's own geometry.
    let bow_world = bow_axis_world(&model).context("could not derive bow axis from skeleton")?;
    println!(
        "  bow axis (world): [{:.2}, {:.2}, {:.2}] (sign {BOW_SIGN:+})",
        bow_world.x, bow_world.y, bow_world.z
    );

    // Layer the bow onto each target bone's animated rotation track. The tilt
    // ramps 0 -> peak -> 0 over the clip (sin over [0, pi]); bones further down
    // the chain add a touch more so the whole upper body bends. The world bow
    // axis is expressed in each bone's local frame (via its inverse-bind), so a
    // post-multiplied delta pitches that bone forward in world space.
    let mut edited = clip.clone();
    let mut touched = Vec::new();
    for (depth, name) in BOW_BONES.iter().enumerate() {
        let Some(&b) = idx.get(name) else { continue };
        let Some(mi) = model_bone(&model, name) else {
            println!("  {name}: not in mesh skeleton; skipping");
            continue;
        };
        // World bow axis -> this bone's local frame.
        let local_axis = model.skeleton.bones[mi]
            .inverse_bind
            .transform_vector(bow_world)
            .normalized();
        let Some(track) = edited.tracks.get_mut(b) else {
            continue;
        };
        let Some(rots) = track.rotations.as_mut() else {
            println!("  {name}: rotation not animated in this clip; skipping");
            continue;
        };
        #[allow(clippy::cast_precision_loss)]
        let peak = BOW_DEGREES * (1.0 + depth as f32 * 0.15) * BOW_SIGN;
        for (f, q) in rots.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let t = if frames > 1 {
                f as f32 / (frames - 1) as f32
            } else {
                0.0
            };
            let amount = peak * (PI * t).sin(); // 0 at the ends, peak mid-clip
            *q = normalize(qmul(*q, axis_angle(local_axis, amount)));
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

    // Eyeball: export the edited clip as a playable animated GLB.
    let mut preview = model.clone();
    preview.animations = vec![nm_clip_to_clip(
        &redec,
        &skel,
        &model.skeleton,
        "reload_bow",
    )];
    let glb = to_glb(&preview)?;
    let glb_path = format!("{out_dir}/yamato_bow_animated.glb");
    std::fs::write(&glb_path, &glb)?;
    println!(
        "wrote {glb_path} ({} bytes) - play it in a glTF viewer",
        glb.len()
    );

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
