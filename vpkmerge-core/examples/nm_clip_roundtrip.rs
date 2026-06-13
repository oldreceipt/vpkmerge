//! Validate the NM pose codec against real Deadlock `.vnmclip_c` files: for every
//! animated clip in a pak, decode `m_compressedPoseData`, re-encode, and check
//! (a) the re-encoded stream + offsets are byte-identical to the original, and
//! (b) a re-decode reproduces the tracks exactly (pose-identical round-trip).
//! Reports any clip where byte-exactness fails (expected to be rare/zero) and the
//! worst-case per-component dequantization delta on the re-decode. Throwaway dev
//! tool backing the committed `morphic/tests/nm_clip.rs` fixtures.
//!
//! Usage: cargo run --release -p vpkmerge-core --example nm_clip_roundtrip -- \
//!     <pak01_dir.vpk> [name-substring-filter] [max-clips]

use anyhow::{Context, Result};
use morphic::model::{decode_nm_clip, decode_pose_stream, encode_compressed_pose};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let filter = args.next().unwrap_or_default();
    let max: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let vpk = valve_pak::open(&pak)?;
    let mut paths: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vnmclip_c") && p.contains(&filter))
        .cloned()
        .collect();
    paths.sort();

    let (mut checked, mut byte_exact, mut pose_exact) = (0usize, 0usize, 0usize);
    let mut byte_fail = Vec::new();
    let mut pose_fail = Vec::new();
    let (mut worst_rot, mut worst_tr, mut worst_sc) = (0f32, 0f32, 0f32);

    for p in &paths {
        if checked >= max {
            break;
        }
        let Ok(bytes) = vpk.get_file(p).and_then(|mut f| f.read_all()) else {
            continue;
        };
        let clip = match decode_nm_clip(&bytes) {
            Ok(c) => c,
            Err(e) => {
                println!("DECODE FAIL {p}: {e}");
                continue;
            }
        };
        // Only animated clips exercise the codec.
        if clip.compressed_pose_data.is_empty() {
            continue;
        }
        checked += 1;

        let (data2, offsets2) = encode_compressed_pose(&clip);
        let bytes_ok =
            data2 == clip.compressed_pose_data && offsets2 == clip.compressed_pose_offsets;
        if bytes_ok {
            byte_exact += 1;
        } else {
            let n = data2.len().min(clip.compressed_pose_data.len());
            let diff = (0..n)
                .filter(|&i| data2[i] != clip.compressed_pose_data[i])
                .count();
            byte_fail.push(format!(
                "{p}: {diff}/{} bytes differ (len {} vs {}, offsets {})",
                clip.compressed_pose_data.len(),
                data2.len(),
                clip.compressed_pose_data.len(),
                if offsets2 == clip.compressed_pose_offsets {
                    "ok"
                } else {
                    "DIFFER"
                }
            ));
        }

        // Pose-identity: re-decode the re-encoded stream against the clip's own
        // track settings; the tracks must match the first decode exactly.
        let settings: Vec<_> = clip.tracks.iter().map(|t| t.settings).collect();
        match decode_pose_stream(&settings, &data2, &offsets2, clip.frame_count) {
            Ok(tracks2) if tracks2 == clip.tracks => pose_exact += 1,
            Ok(tracks2) => {
                let (rot, tr, sc) = max_track_delta(&clip.tracks, &tracks2);
                worst_rot = worst_rot.max(rot);
                worst_tr = worst_tr.max(tr);
                worst_sc = worst_sc.max(sc);
                pose_fail.push(format!("{p} (rot {rot:.4} tr {tr:.4} sc {sc:.4})"));
            }
            Err(e) => pose_fail.push(format!("{p}: decode err {e}")),
        }
    }

    println!("\nchecked {checked} animated clips matching {filter:?}");
    println!("  byte-identical re-encode: {byte_exact}/{checked}");
    println!("  pose-identical round-trip: {pose_exact}/{checked}");
    if !byte_fail.is_empty() {
        println!("\n{} byte-exact failures:", byte_fail.len());
        for f in byte_fail.iter().take(25) {
            println!("  {f}");
        }
    }
    if !pose_fail.is_empty() {
        println!(
            "\n{} pose round-trip drifts; worst-case deltas: rotation(angle rad) {worst_rot:.5} \
             translation {worst_tr:.5} scale {worst_sc:.5}",
            pose_fail.len()
        );
        for f in pose_fail.iter().take(15) {
            println!("  {f}");
        }
    }
    Ok(())
}

/// Worst per-component delta between two decodes of the same clip: rotation as
/// the geodesic angle between the unit quaternions (radians, sign-agnostic),
/// translation and scale as absolute differences.
fn max_track_delta(
    a: &[morphic::model::NmTrack],
    b: &[morphic::model::NmTrack],
) -> (f32, f32, f32) {
    let (mut rot, mut tr, mut sc) = (0f32, 0f32, 0f32);
    for (ta, tb) in a.iter().zip(b.iter()) {
        if let (Some(ra), Some(rb)) = (&ta.rotations, &tb.rotations) {
            for (qa, qb) in ra.iter().zip(rb.iter()) {
                let dot = (qa.x * qb.x + qa.y * qb.y + qa.z * qb.z + qa.w * qb.w).abs();
                rot = rot.max(2.0 * dot.clamp(0.0, 1.0).acos());
            }
        }
        if let (Some(va), Some(vb)) = (&ta.translations, &tb.translations) {
            for (pa, pb) in va.iter().zip(vb.iter()) {
                tr = tr.max(
                    (pa.x - pb.x)
                        .abs()
                        .max((pa.y - pb.y).abs())
                        .max((pa.z - pb.z).abs()),
                );
            }
        }
        if let (Some(va), Some(vb)) = (&ta.scales, &tb.scales) {
            for (a, b) in va.iter().zip(vb.iter()) {
                sc = sc.max((a - b).abs());
            }
        }
    }
    (rot, tr, sc)
}
