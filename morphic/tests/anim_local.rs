//! Local (non-CI) animation-decode check: the embedded-animation path needs the
//! multi-megabyte `ANIM`/`ASEQ`/`AGRP` blocks, so it is gated on
//! `MORPHIC_MODEL_VPK` pointing at a Deadlock `pak01_dir.vpk` and skipped
//! otherwise. Decodes the hornet hero model's clips and diffs clip set + per-clip
//! fps, frame count, and looping flag (exact) plus sampled per-bone keyframes
//! (within tolerance) against the committed oracle golden `hornet_anim_meta.json`
//! (produced by `morphic-oracle anim-meta`, wrapping `ValveResourceFormat`).
//!
//! The raw animation blocks are NOT committed (~16 MB); only the small JSON
//! golden is, so the heavy correctness check runs against a live install.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct Golden {
    clip_count: usize,
    clips: Vec<GClip>,
    samples: Vec<GSample>,
}

#[derive(Deserialize)]
struct GClip {
    name: String,
    fps: f32,
    frame_count: usize,
    looping: bool,
}

#[derive(Deserialize)]
struct GSample {
    clip: String,
    bone: String,
    channel: String,
    frame: usize,
    value: Vec<f32>,
}

fn load_golden() -> Golden {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/kv3/hornet_anim_meta.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read anim golden"))
        .expect("parse anim golden")
}

/// The morphic transform for one (bone, channel) at a frame: the track value if
/// the clip animates that channel, else the bind pose (matching VRF's
/// `Frame.Clear`, which an untouched bone keeps).
fn morphic_value(
    model: &morphic::model::Model,
    clip: &morphic::model::Clip,
    bone: &str,
    channel: &str,
    frame: usize,
) -> Vec<f32> {
    let idx = model
        .skeleton
        .bones
        .iter()
        .position(|b| b.name == bone)
        .unwrap_or_else(|| panic!("bone {bone} not in skeleton"));
    let bind = &model.skeleton.bones[idx];
    let track = clip.tracks.iter().find(|t| t.bone == idx);
    match channel {
        "translation" => track.and_then(|t| t.translations.as_ref()).map_or_else(
            || vec![bind.position.x, bind.position.y, bind.position.z],
            |v| vec![v[frame].x, v[frame].y, v[frame].z],
        ),
        "rotation" => track.and_then(|t| t.rotations.as_ref()).map_or_else(
            || {
                vec![
                    bind.rotation.x,
                    bind.rotation.y,
                    bind.rotation.z,
                    bind.rotation.w,
                ]
            },
            |v| vec![v[frame].x, v[frame].y, v[frame].z, v[frame].w],
        ),
        "scale" => track
            .and_then(|t| t.scales.as_ref())
            .map_or_else(|| vec![1.0], |v| vec![v[frame]]),
        other => panic!("unknown channel {other}"),
    }
}

/// Per-component closeness; for `rotation`, allow a global sign flip (q and -q
/// are the same orientation).
fn values_match(a: &[f32], b: &[f32], rotation: bool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let same = a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3);
    if same || !rotation {
        return same;
    }
    a.iter().zip(b).all(|(x, y)| (x + y).abs() < 2e-3)
}

#[test]
fn hornet_animations_match_golden() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local animation decode");
        return;
    };
    let entry = std::env::var("MORPHIC_MODEL_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("locate entry");
    let bytes = vf.read_all().expect("read entry");
    let model = morphic::model::decode(&bytes).expect("decode model");

    let golden = load_golden();

    // Clip set: same count and same names (the emitted clips, frame_count >= 1).
    let emitted: Vec<&morphic::model::Clip> = model
        .animations
        .iter()
        .filter(|c| c.frame_count >= 1)
        .collect();
    assert_eq!(
        emitted.len(),
        golden.clip_count,
        "clip count: morphic {} vs golden {}",
        emitted.len(),
        golden.clip_count
    );

    let mut got_names: Vec<&str> = emitted.iter().map(|c| c.name.as_str()).collect();
    let mut want_names: Vec<&str> = golden.clips.iter().map(|c| c.name.as_str()).collect();
    got_names.sort_unstable();
    want_names.sort_unstable();
    assert_eq!(got_names, want_names, "clip name set differs");

    // Per-clip fps / frame_count / looping (exact).
    let by_name: std::collections::HashMap<&str, &morphic::model::Clip> =
        emitted.iter().map(|c| (c.name.as_str(), *c)).collect();
    for g in &golden.clips {
        let c = by_name
            .get(g.name.as_str())
            .unwrap_or_else(|| panic!("clip {} missing", g.name));
        assert!(
            (c.fps - g.fps).abs() < 1e-3,
            "clip {} fps: {} vs {}",
            g.name,
            c.fps,
            g.fps
        );
        assert_eq!(c.frame_count, g.frame_count, "clip {} frame_count", g.name);
        assert_eq!(c.looping, g.looping, "clip {} looping", g.name);
    }

    // Sampled keyframes: position / packed-quaternion angle / scale decoders.
    assert!(!golden.samples.is_empty(), "golden has samples");
    for s in &golden.samples {
        let clip = by_name
            .get(s.clip.as_str())
            .unwrap_or_else(|| panic!("sample clip {} missing", s.clip));
        let got = morphic_value(&model, clip, &s.bone, &s.channel, s.frame);
        assert!(
            values_match(&got, &s.value, s.channel == "rotation"),
            "{} / {} / {} @ frame {}: morphic {:?} vs golden {:?}",
            s.clip,
            s.bone,
            s.channel,
            s.frame,
            got,
            s.value
        );
    }

    eprintln!(
        "anim_local: {} clips, {} samples matched",
        emitted.len(),
        golden.samples.len()
    );
}
