//! Unmistakable in-game confirmation of the full v4 clip encoder, on a DIFFERENT
//! animation than the reload: turn Yamato's *static* stand-idle clips into a
//! full-body sway, so she visibly moves while just standing in the hideout. This
//! is the hardest case for `reencode_nm_clip_full` -- it creates the pose blob
//! from scratch (the idle clips ship fully static, `m_compressedPoseData` empty)
//! and animates every bone -- and it's clearly distinct from the reload wobble
//! (different slot, full body, idle context). If she sways while idle, the full v4
//! encoder is engine-confirmed.
//!
//! Usage: cargo run --release -p vpkmerge-core --example idle_dance -- <pak01_dir.vpk> <out_dir>

use std::f32::consts::TAU;

use anyhow::{Context, Result};
use morphic::model::{
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, reencode_nm_clip_full, to_glb,
    Quat,
};

const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
// Stand-idle slots (all ship fully static); animate each so whichever the graph
// picks while standing shows the sway.
const IDLES: &[&str] = &[
    "models/heroes_wip/yamato/clips/hideout_stand_idle.vnmclip_c",
    "models/heroes_wip/yamato/clips/out_of_combat_stand_idle.vnmclip_c",
    "models/heroes_wip/yamato/clips/weapon_stand_idle.vnmclip_c",
    "models/heroes_wip/yamato/clips/item_stand_idle.vnmclip_c",
];
const AMP_DEG: f32 = 35.0;

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

/// Big phase-shifted sway about local Z (one cycle over the clip).
fn sway(track: usize, f: usize, frames: usize) -> Quat {
    #[allow(clippy::cast_precision_loss)]
    let u = if frames > 1 {
        f as f32 / (frames - 1) as f32
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)]
    let deg = AMP_DEG * (TAU * u + track as f32 * 0.5).sin();
    let h = deg.to_radians() * 0.5;
    Quat {
        x: 0.0,
        y: 0.0,
        z: h.sin(),
        w: h.cos(),
    }
}

/// Animate every bone's rotation of a (static) clip and full-re-encode it.
fn dance(bytes: &[u8]) -> Result<(morphic::model::NmClip, Vec<u8>)> {
    let clip = decode_nm_clip(bytes)?;
    let frames = clip.frame_count.max(1) as usize;
    let mut edited = clip.clone();
    for (ti, t) in edited.tracks.iter_mut().enumerate() {
        let base = t.settings.constant_rotation;
        t.rotations = Some(
            (0..frames)
                .map(|f| normalize(qmul(base, sway(ti, f, frames))))
                .collect(),
        );
    }
    let out = reencode_nm_clip_full(bytes, &edited).context("full re-encode idle dance")?;
    Ok((decode_nm_clip(&out)?, out))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("pak")?;
    let out_dir = args.next().context("out_dir")?;

    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let model = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let mut preview_written = false;
    for entry in IDLES {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            println!("  skip (not found): {entry}");
            continue;
        };
        let before = decode_nm_clip(&bytes)?;
        let (redec, out) = dance(&bytes)?;
        let animated = redec
            .tracks
            .iter()
            .filter(|t| t.rotations.is_some())
            .count();
        println!(
            "  {entry}: {} frames, blob {} -> {} bytes, {animated} animated rotation tracks (was {})",
            redec.frame_count,
            before.compressed_pose_data.len(),
            redec.compressed_pose_data.len(),
            before.tracks.iter().filter(|t| t.rotations.is_some()).count(),
        );
        if !preview_written {
            let mut m = model.clone();
            m.animations = vec![nm_clip_to_clip(
                &redec,
                &skel,
                &model.skeleton,
                "idle_dance",
            )];
            std::fs::write(format!("{out_dir}/yamato_idle_dance.glb"), to_glb(&m)?)?;
            preview_written = true;
        }
        packed.push(((*entry).to_string(), out));
    }
    anyhow::ensure!(!packed.is_empty(), "no idle clips edited");

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let out_vpk = format!("{out_dir}/yamato_idle_dance_dir.vpk");
    vpkmerge_core::pack(&refs, &out_vpk)?;
    println!("\npacked {} idle clips -> {out_vpk}", packed.len());
    println!("stand still (hideout): PASS = whole body sways; FAIL = stands static / breaks.");
    Ok(())
}
