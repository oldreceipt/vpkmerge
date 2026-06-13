//! In-game test for the NM clip *encoder* (`reencode_nm_clip`): the unverified new
//! capability is adding animated channels (a static bone becomes animated), which
//! flips `m_bIsRotationStatic`, grows the pose blob, and rewrites the offsets. To
//! make the pass/fail obvious, this animates **every** rotation bone of Yamato's
//! `reload_idle_quick` with a big full-body wobble: vanilla reload moves only her
//! arms, so if the encoder works her entire body (legs, head, fingers) thrashes,
//! and if it doesn't the reload looks normal (channels ignored) or she breaks.
//!
//! Writes an animated GLB to eyeball first, then packs at the reload paths.
//! Usage: cargo run --release -p vpkmerge-core --example encoder_test -- \
//!     <pak01_dir.vpk> <out_dir>

use std::f32::consts::TAU;

use anyhow::{Context, Result};
use morphic::model::{
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, reencode_nm_clip, to_glb, Quat,
};

const CLIP: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const RELOAD_IDLE: &str = "models/heroes_wip/yamato/clips/reload_idle.vnmclip_c";
const RELOAD_IDLE_QUICK: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

const AMP_DEG: f32 = 50.0;
const CYCLES: f32 = 2.0;

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

/// A per-bone, per-frame wobble about local Z (each bone phase-shifted).
fn wobble(track: usize, f: usize, frames: usize) -> Quat {
    #[allow(clippy::cast_precision_loss)]
    let u = if frames > 1 {
        f as f32 / (frames - 1) as f32
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)]
    let phase = track as f32 * 0.6;
    let deg = AMP_DEG * (CYCLES * TAU * u + phase).sin();
    let h = deg.to_radians() * 0.5;
    Quat {
        x: 0.0,
        y: 0.0,
        z: h.sin(),
        w: h.cos(),
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: out_dir")?;
    // "full" -> route through reencode_nm_clip_full (whole-DATA v4 re-encode) to
    // confirm that path plays in-engine; otherwise the in-place v5 surgical path.
    let full = args.next().as_deref() == Some("full");

    let bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let clip = decode_nm_clip(&bytes)?;
    let frames = clip.frame_count as usize;
    let static_before = clip.tracks.iter().filter(|t| t.rotations.is_none()).count();

    // Drive every bone's rotation: existing animated tracks get the wobble layered
    // on; static ones become animated (constant rotation + wobble).
    let mut edited = clip.clone();
    for (ti, track) in edited.tracks.iter_mut().enumerate() {
        let base = track.settings.constant_rotation;
        let rots: Vec<Quat> = (0..frames)
            .map(|f| {
                let from = track
                    .rotations
                    .as_ref()
                    .and_then(|r| r.get(f).copied())
                    .unwrap_or(base);
                normalize(qmul(from, wobble(ti, f, frames)))
            })
            .collect();
        track.rotations = Some(rots);
    }
    let animated_after = edited
        .tracks
        .iter()
        .filter(|t| t.rotations.is_some())
        .count();
    println!(
        "reload_idle_quick: {frames} frames, {} bones | rotation tracks: {} animated -> {animated_after} \
         (added {static_before})",
        clip.tracks.len(),
        clip.tracks.len() - static_before,
    );

    let out = if full {
        println!("(full v4 re-encode path)");
        morphic::model::reencode_nm_clip_full(&bytes, &edited).context("reencode_nm_clip_full")?
    } else {
        reencode_nm_clip(&bytes, &edited).context("reencode (add all rotation channels)")?
    };
    let redec = decode_nm_clip(&out).context("re-decode reencoded clip")?;
    let now_animated = redec
        .tracks
        .iter()
        .filter(|t| t.rotations.is_some())
        .count();
    println!(
        "re-decode OK: blob {} -> {} bytes, animated rotation tracks now {now_animated}",
        clip.compressed_pose_data.len(),
        redec.compressed_pose_data.len(),
    );
    anyhow::ensure!(
        now_animated == clip.tracks.len(),
        "expected every rotation track animated after reencode"
    );

    // Animated GLB to eyeball the whole-body wobble before installing.
    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let mut preview = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
    preview.animations = vec![nm_clip_to_clip(
        &redec,
        &skel,
        &preview.skeleton,
        "encoder_test",
    )];
    let glb = to_glb(&preview)?;
    let glb_path = format!("{out_dir}/yamato_encoder_test.glb");
    std::fs::write(&glb_path, &glb)?;
    println!(
        "wrote {glb_path} ({} bytes) - whole body should wobble",
        glb.len()
    );

    let out_vpk = format!("{out_dir}/yamato_encoder_test_dir.vpk");
    vpkmerge_core::pack(
        &[
            (RELOAD_IDLE, out.as_slice()),
            (RELOAD_IDLE_QUICK, out.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked -> {out_vpk}");
    println!(
        "install as a free pakNN_dir.vpk, press R: PASS = whole body thrashes (legs/head move),"
    );
    println!("FAIL = only arms move (channels ignored) or she breaks (clip rejected).");
    Ok(())
}
