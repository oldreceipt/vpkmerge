//! Committed-fixture regression for the NM quantized-pose codec
//! (`m_compressedPoseData`). Reads four small real Deadlock `.vnmclip_c` files
//! under `fixtures/nm/` (extracted from yamato's clip set; see `fixtures/README.md`)
//! and asserts:
//!  - the animated clips decode their compressed pose stream into per-bone
//!    rotation/translation tracks, re-encode **byte-identically**, and re-decode
//!    to the same tracks (the decode -> encode -> decode round-trip);
//!  - the static menu-pose clip decodes with no compressed stream (every channel
//!    constant), the format's degenerate case;
//!  - decoded rotations are unit quaternions (the decode reconstructs the dropped
//!    component correctly).
//!
//! These three fixtures are in the ~91% of pak clips whose smallest-three
//! quaternions have an unambiguous largest component, so the re-encode is exact.
//! The remaining clips round-trip pose-identically (rotation within ~0.001 rad,
//! translation/scale exact) but not always byte-for-byte, an inherent property of
//! the lossy packing; that broad guarantee is checked in the gated
//! `tests/nm_clip_local.rs` against a full pak. The animated-scale channel (rare;
//! no yamato clip uses it) is covered by the `frame_stream_round_trips` unit test
//! in `model::nm`.

use morphic::model::{decode_nm_clip, decode_pose_stream, encode_compressed_pose, NmClip};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/fixtures/nm/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"))
}

/// Decoded quaternions must be (near) unit length: a sanity check that the
/// dropped-component reconstruction is correct.
fn assert_rotations_unit(clip: &NmClip) {
    for tr in &clip.tracks {
        if let Some(rots) = &tr.rotations {
            for q in rots {
                let len = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
                assert!((len - 1.0).abs() < 1e-3, "non-unit rotation len {len}");
            }
        }
    }
}

/// The core property: decode -> encode is byte-identical, and decode -> encode ->
/// decode reproduces the tracks exactly.
fn assert_byte_exact_round_trip(name: &str) -> NmClip {
    let bytes = fixture(name);
    let clip = decode_nm_clip(&bytes).unwrap_or_else(|e| panic!("decode {name}: {e}"));
    assert!(
        !clip.compressed_pose_data.is_empty(),
        "{name} should be an animated clip with a pose stream"
    );

    let (data2, offsets2) = encode_compressed_pose(&clip);
    assert_eq!(
        data2, clip.compressed_pose_data,
        "{name}: re-encoded pose stream is not byte-identical"
    );
    assert_eq!(
        offsets2, clip.compressed_pose_offsets,
        "{name}: re-encoded frame offsets differ"
    );

    let settings: Vec<_> = clip.tracks.iter().map(|t| t.settings).collect();
    let tracks2 = decode_pose_stream(&settings, &data2, &offsets2, clip.frame_count)
        .unwrap_or_else(|e| panic!("re-decode {name}: {e}"));
    assert_eq!(tracks2, clip.tracks, "{name}: re-decode changed the tracks");

    assert_rotations_unit(&clip);
    clip
}

#[test]
fn rope_climb_idle_round_trips() {
    // Translation-only animated clip (9 frames, 6 animated translation tracks).
    let clip = assert_byte_exact_round_trip("yamato_rope_climb_idle.vnmclip_c");
    assert_eq!(clip.frame_count, 9);
    assert!(!clip.additive);
    let with_trans = clip
        .tracks
        .iter()
        .filter(|t| t.translations.is_some())
        .count();
    let with_rot = clip.tracks.iter().filter(|t| t.rotations.is_some()).count();
    assert_eq!(with_trans, 6, "expected 6 animated translation tracks");
    assert_eq!(with_rot, 0, "rope_climb_idle animates no rotations");
    // Each present translation track carries exactly frame_count samples.
    for tr in &clip.tracks {
        if let Some(t) = &tr.translations {
            assert_eq!(t.len(), clip.frame_count as usize);
        }
    }
}

#[test]
fn reload_idle_quick_round_trips() {
    // Rotation + translation mix (21 frames); the proven press-R taunt slot.
    let clip = assert_byte_exact_round_trip("yamato_reload_idle_quick.vnmclip_c");
    assert_eq!(clip.frame_count, 21);
    assert!(!clip.additive);
    assert!(
        clip.tracks.iter().any(|t| t.rotations.is_some()),
        "has animated rotation"
    );
    assert!(
        clip.tracks.iter().any(|t| t.translations.is_some()),
        "has animated translation"
    );
    for tr in &clip.tracks {
        if let Some(r) = &tr.rotations {
            assert_eq!(r.len(), clip.frame_count as usize);
        }
    }
}

#[test]
fn flinch_back_additive_round_trips() {
    // Additive clip: same codec, additive flag set.
    let clip = assert_byte_exact_round_trip("yamato_flinch_back.vnmclip_c");
    assert_eq!(clip.frame_count, 15);
    assert!(clip.additive, "flinch_back is an additive clip");
}

#[test]
fn ui_hero_select_is_fully_static() {
    // The named first target: a single authored menu pose, every track constant,
    // no compressed stream. Decodes cleanly with all channel vectors empty.
    let bytes = fixture("yamato_ui_hero_select.vnmclip_c");
    let clip = decode_nm_clip(&bytes).expect("decode ui_hero_select");
    assert!(
        clip.compressed_pose_data.is_empty(),
        "ui_hero_select carries no compressed pose data"
    );
    assert_eq!(clip.frame_count, 10);
    assert!(
        clip.tracks
            .iter()
            .all(|t| t.rotations.is_none() && t.translations.is_none() && t.scales.is_none()),
        "every track of a static clip is constant (no animated channel vectors)"
    );
    assert!(
        !clip.tracks.is_empty(),
        "static clip still has per-bone tracks"
    );
    // Re-encode of a static clip is an empty stream with one zero offset per frame.
    let (data2, offsets2) = encode_compressed_pose(&clip);
    assert!(data2.is_empty());
    assert_eq!(offsets2, vec![0u32; clip.frame_count as usize]);
}
