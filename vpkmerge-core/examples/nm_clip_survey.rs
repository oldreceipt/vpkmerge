//! Survey `.vnmclip_c` entries in a pak: per clip report frame count, compressed
//! pose-data length, and how many tracks are static vs animated. Used to pick an
//! animated clip (non-empty `m_compressedPoseData`) for the pose-codec round-trip
//! fixture, and to confirm yamato's `ui_hero_select` is the single-frame static
//! sanity target. Throwaway dev tool.
//!
//! Usage: cargo run --release -p vpkmerge-core --example nm_clip_survey -- \
//!     <pak01_dir.vpk> [name-substring-filter]

use anyhow::{Context, Result};
use morphic::kv3::Value;

/// Returns (fully-static tracks, tracks with any animated channel, animated
/// rotations, animated translations, animated scales).
fn track_stats(tracks: &[Value]) -> (usize, usize, usize, usize, usize) {
    let (mut stat, mut anim, mut ar, mut at, mut asc) = (0, 0, 0, 0, 0);
    for t in tracks {
        let is = |k: &str| t.get(k).and_then(Value::as_bool).unwrap_or(false);
        let (rs, ts, ss) = (
            is("m_bIsRotationStatic"),
            is("m_bIsTranslationStatic"),
            is("m_bIsScaleStatic"),
        );
        if rs && ts && ss {
            stat += 1;
        } else {
            anim += 1;
        }
        ar += usize::from(!rs);
        at += usize::from(!ts);
        asc += usize::from(!ss);
    }
    (stat, anim, ar, at, asc)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let filter = args.next().unwrap_or_default();

    let vpk = valve_pak::open(&pak)?;
    let mut paths: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vnmclip_c") && p.contains(&filter))
        .cloned()
        .collect();
    paths.sort();
    println!("{} .vnmclip_c entries matching {filter:?}", paths.len());

    let mut animated = Vec::new();
    for p in &paths {
        let Ok(bytes) = vpk.get_file(p).and_then(|mut f| f.read_all()) else {
            continue;
        };
        let Ok(root) = morphic::decode_kv3_resource(&bytes) else {
            println!("  {p}: KV3 decode FAILED");
            continue;
        };
        let frames = root
            .get("m_nNumFrames")
            .and_then(|v| {
                v.as_uint()
                    .or_else(|| v.as_int().and_then(|n| u64::try_from(n).ok()))
            })
            .unwrap_or(0);
        let pose_len = match root.get("m_compressedPoseData") {
            Some(Value::Binary(b)) => b.len(),
            _ => 0,
        };
        let offsets = root
            .get("m_compressedPoseOffsets")
            .and_then(Value::as_array)
            .map_or(0, <[Value]>::len);
        let (stat, anim, ar, at, asc) = root
            .get("m_trackCompressionSettings")
            .and_then(Value::as_array)
            .map_or((0, 0, 0, 0, 0), |a| track_stats(a));
        let additive = root
            .get("m_bIsAdditive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if pose_len > 0 && anim > 0 {
            println!(
                "  [ANIM] {} bytes  frames={frames} pose={pose_len}B offsets={offsets} \
                 static={stat} anim={anim} (rot={ar} trans={at} scale={asc}) additive={additive}  {p}",
                bytes.len()
            );
            animated.push((p.clone(), bytes.len(), frames, pose_len, stat, anim));
        } else if filter.is_empty() {
            // keep static-clip noise down unless a filter was given
        } else {
            println!(
                "  [stat] {} bytes  frames={frames} pose={pose_len}B static={stat} anim={anim}  {p}",
                bytes.len()
            );
        }
    }

    println!("\n{} animated clips found", animated.len());
    animated.sort_by_key(|c| c.1);
    for (p, sz, frames, pose, stat, anim) in animated.iter().take(20) {
        println!(
            "  {sz:>8}B  frames={frames:<4} pose={pose:<8} static={stat:<4} anim={anim:<4} {p}"
        );
    }
    Ok(())
}
