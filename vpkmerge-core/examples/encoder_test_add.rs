//! Minimal channel-add test for `reencode_nm_clip`: animate only the *static*
//! bones whose name contains a filter (default "finger") in Yamato's
//! `reload_idle_quick`, curling them over the reload. Bisects the failure of the
//! all-93-bones test: if even this small, isolated channel-add (no IK/root/spine)
//! breaks the reload, the engine's animated-channel set is baked at compile time
//! and the clip's `m_bIsRotationStatic` flags are not the source of truth (so
//! channels cannot be added, only existing ones edited). If the fingers curl and
//! the rest of the reload plays normally, channel-adds work and the big test
//! failed for another reason.
//!
//! Usage: cargo run --release -p vpkmerge-core --example encoder_test_add -- \
//!     <pak01_dir.vpk> <out_dir> [name-filter=finger]

use anyhow::{Context, Result};
use morphic::model::{
    decode, decode_nm_clip, decode_nm_skeleton, nm_clip_to_clip, reencode_nm_clip, to_glb, Quat,
};

const CLIP: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";
const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const MESH: &str = "models/heroes_staging/yamato_v2/yamato.vmdl_c";
const RELOAD_IDLE: &str = "models/heroes_wip/yamato/clips/reload_idle.vnmclip_c";
const RELOAD_IDLE_QUICK: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

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

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("pak")?;
    let out_dir = args.next().context("out_dir")?;
    let filter = args.next().unwrap_or_else(|| "finger".to_string());

    let skel = decode_nm_skeleton(&vpkmerge_core::read_vpk_entry(&pak, SKEL)?)?;
    let bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let clip = decode_nm_clip(&bytes)?;
    let frames = clip.frame_count as usize;

    // Curl every static-rotation bone matching the filter (e.g. fingers) about
    // local X, ramping to ~80 degrees over the clip.
    let mut edited = clip.clone();
    let mut hit = Vec::new();
    for (i, track) in edited.tracks.iter_mut().enumerate() {
        let name = skel.bone_names.get(i).map_or("", String::as_str);
        if track.rotations.is_some() || !name.to_ascii_lowercase().contains(&filter) {
            continue; // only newly animate matching static-rotation bones
        }
        let base = track.settings.constant_rotation;
        let rots: Vec<Quat> = (0..frames)
            .map(|f| {
                #[allow(clippy::cast_precision_loss)]
                let u = if frames > 1 {
                    f as f32 / (frames - 1) as f32
                } else {
                    0.0
                };
                let h = (80.0_f32.to_radians() * u) * 0.5; // curl about local X
                let curl = Quat {
                    x: h.sin(),
                    y: 0.0,
                    z: 0.0,
                    w: h.cos(),
                };
                normalize(qmul(base, curl))
            })
            .collect();
        track.rotations = Some(rots);
        hit.push(name.to_string());
    }
    anyhow::ensure!(!hit.is_empty(), "no static bone matched {filter:?}");
    println!(
        "adding rotation to {} static bones matching {filter:?}: {hit:?}",
        hit.len()
    );

    let out = reencode_nm_clip(&bytes, &edited).context("reencode (minimal channel add)")?;
    let redec = decode_nm_clip(&out).context("re-decode")?;
    println!(
        "re-decode OK: blob {} -> {} bytes",
        clip.compressed_pose_data.len(),
        redec.compressed_pose_data.len()
    );

    let mut preview = decode(&vpkmerge_core::read_vpk_entry(&pak, MESH)?)?;
    preview.animations = vec![nm_clip_to_clip(
        &redec,
        &skel,
        &preview.skeleton,
        "add_test",
    )];
    let glb = to_glb(&preview)?;
    let glb_path = format!("{out_dir}/yamato_add_{filter}.glb");
    std::fs::write(&glb_path, &glb)?;
    println!("wrote {glb_path} ({} bytes)", glb.len());

    let out_vpk = format!("{out_dir}/yamato_add_{filter}_dir.vpk");
    vpkmerge_core::pack(
        &[
            (RELOAD_IDLE, out.as_slice()),
            (RELOAD_IDLE_QUICK, out.as_slice()),
        ],
        &out_vpk,
    )?;
    println!("\npacked -> {out_vpk}");
    println!("press R: PASS = {filter}s curl + rest of reload normal; FAIL = reload breaks/none.");
    Ok(())
}
