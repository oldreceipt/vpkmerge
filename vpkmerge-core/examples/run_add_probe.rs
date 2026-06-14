//! Decisive probe: does the engine read ADDED animated channels, or only a
//! compile-baked channel set? Adds finger-curl rotation channels (the fingers are
//! static in a run) to Yamato's `weapon_run_*` clips via the proven **v5 in-place**
//! path (`reencode_nm_clip`), on an UNMASKED full-body slot (locomotion), so the
//! result isn't confounded by a bone mask like the reload was.
//!
//! Run around in the hideout: PASS (channel-adds work) = her fists clench/release
//! each stride; FAIL (channel set is baked) = hands stay open while the run plays
//! normally.
//!
//! Usage: cargo run --release -p vpkmerge-core --example run_add_probe -- <pak01_dir.vpk> <out_dir>

use std::f32::consts::PI;

use anyhow::{Context, Result};
use morphic::model::{decode_nm_clip, decode_nm_skeleton, reencode_nm_clip, Quat};

const SKEL: &str = "models/heroes_wip/yamato/yamato.vnmskel_c";
const DIRS: &[&str] = &["center", "n", "ne", "e", "se", "s", "sw", "w", "nw"];

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

    if filter == "list" {
        let bytes = vpkmerge_core::read_vpk_entry(
            &pak,
            "models/heroes_wip/yamato/clips/weapon_run_n.vnmclip_c",
        )?;
        let clip = decode_nm_clip(&bytes)?;
        println!("weapon_run_n static-rotation bones:");
        for (i, t) in clip.tracks.iter().enumerate() {
            if t.rotations.is_none() {
                println!(
                    "  {i:>3} {}",
                    skel.bone_names.get(i).map_or("?", String::as_str)
                );
            }
        }
        return Ok(());
    }

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let mut reported = false;
    for dir in DIRS {
        let entry = format!("models/heroes_wip/yamato/clips/weapon_run_{dir}.vnmclip_c");
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, &entry) else {
            continue;
        };
        let clip = decode_nm_clip(&bytes)?;
        let frames = clip.frame_count.max(1) as usize;

        let mut edited = clip.clone();
        let mut added = Vec::new();
        for (i, t) in edited.tracks.iter_mut().enumerate() {
            let name = skel.bone_names.get(i).map_or("", String::as_str);
            // Only newly-animate static-rotation finger bones (a genuine channel add
            // on an unmasked region).
            if t.rotations.is_some() || !name.to_ascii_lowercase().contains(&filter) {
                continue;
            }
            let base = t.settings.constant_rotation;
            t.rotations = Some(
                (0..frames)
                    .map(|f| {
                        #[allow(clippy::cast_precision_loss)]
                        let u = f as f32 / (frames.max(2) - 1) as f32;
                        // swing out and back each loop (0 -> 120deg -> 0).
                        let deg = 120.0 * (PI * u).sin();
                        let h = deg.to_radians() * 0.5;
                        normalize(qmul(
                            base,
                            Quat {
                                x: h.sin(),
                                y: 0.0,
                                z: 0.0,
                                w: h.cos(),
                            },
                        ))
                    })
                    .collect(),
            );
            added.push(name.to_string());
        }
        if added.is_empty() {
            continue;
        }
        let out = reencode_nm_clip(&bytes, &edited).with_context(|| format!("reencode {entry}"))?;
        if !reported {
            println!(
                "weapon_run_{dir}: added {} finger channels, blob {} -> {} bytes",
                added.len(),
                clip.compressed_pose_data.len(),
                decode_nm_clip(&out)?.compressed_pose_data.len()
            );
            reported = true;
        }
        packed.push((entry, out));
    }
    anyhow::ensure!(!packed.is_empty(), "no run clips edited");

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let out_vpk = format!("{out_dir}/yamato_run_finger_add_dir.vpk");
    vpkmerge_core::pack(&refs, &out_vpk)?;
    println!("packed {} run clips -> {out_vpk}", packed.len());
    println!("run around: PASS = fists clench/release each stride; FAIL = hands open, run normal.");
    Ok(())
}
