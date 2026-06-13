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

use morphic::model::{decode_nm_clip, decode_pose_stream, encode_compressed_pose, NmClip, Quat};

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
fn animated_edit_splices_back_into_the_resource() {
    // The full animated-edit pipeline: decode -> edit a rotation track across all
    // frames -> re-encode the (equal-length) pose stream -> splice it back into the
    // resource byte-faithfully -> re-decode. The edit must survive verbatim and no
    // other track may move. Exercises the blobbed-LZ4 v5 single-frame blob splice
    // (`patch_kv3_resource_blob`), the engine-loadable path for editing motion.
    let bytes = fixture("yamato_reload_idle_quick.vnmclip_c");
    let clip = decode_nm_clip(&bytes).expect("decode");

    // Pick the first track with an animated rotation and rotate every frame by a
    // fixed delta about Z (compose: q * delta).
    let target = clip
        .tracks
        .iter()
        .position(|t| t.rotations.is_some())
        .expect("clip has an animated rotation");
    let delta = {
        let half = 20.0_f32.to_radians() * 0.5;
        Quat {
            x: 0.0,
            y: 0.0,
            z: half.sin(),
            w: half.cos(),
        }
    };
    let qmul = |a: Quat, b: Quat| Quat {
        w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
    };

    let mut edited = clip.clone();
    for q in edited.tracks[target].rotations.as_mut().unwrap() {
        let n = qmul(*q, delta);
        let len = (n.x * n.x + n.y * n.y + n.z * n.z + n.w * n.w).sqrt();
        *q = Quat {
            x: n.x / len,
            y: n.y / len,
            z: n.z / len,
            w: n.w / len,
        };
    }

    let (new_blob, new_offsets) = encode_compressed_pose(&edited);
    assert_eq!(
        new_blob.len(),
        clip.compressed_pose_data.len(),
        "editing existing channels must not change the stream length"
    );
    assert_eq!(new_offsets, clip.compressed_pose_offsets);

    let patched = morphic::patch_kv3_resource_blob(&bytes, &clip.compressed_pose_data, &new_blob)
        .expect("splice edited pose stream into the resource");

    // The patched resource must decode to exactly what re-decoding the new stream
    // gives (the splice was byte-faithful): the edited track quantized back, every
    // other track identical to the original.
    let redec = decode_nm_clip(&patched).expect("re-decode patched resource");
    assert_eq!(redec.frame_count, clip.frame_count);
    let settings: Vec<_> = clip.tracks.iter().map(|t| t.settings).collect();
    let reference = decode_pose_stream(&settings, &new_blob, &new_offsets, clip.frame_count)
        .expect("decode re-encoded stream");
    assert_eq!(
        redec.tracks, reference,
        "patched resource decodes to the new stream"
    );

    // The 20-degree edit changed the target track (well beyond quantization noise)
    // and left every other track byte-for-byte unchanged.
    assert_ne!(
        redec.tracks[target].rotations, clip.tracks[target].rotations,
        "edit must take effect"
    );
    for (i, (a, b)) in clip.tracks.iter().zip(redec.tracks.iter()).enumerate() {
        if i != target {
            assert_eq!(a, b, "non-target track {i} must be unchanged");
        }
    }
    assert!(
        !redec.compressed_pose_data.is_empty(),
        "still a valid animated clip"
    );
}

#[test]
fn sole_blob_resize_round_trips() {
    // The container can write a pose blob of a DIFFERENT length (single frame):
    // extend reload_idle_quick's blob, write it back, and confirm the resource
    // re-reads exactly the new bytes. This exercises the per-blob length,
    // sizeBlobs, the frame-size table, comp2, and the header-size updates — if any
    // were wrong the re-read blob would be the wrong length or corrupt. The frame
    // offsets are unchanged, so the (longer) blob's trailing bytes are never read
    // by the track decoder and the original tracks decode identically.
    let bytes = fixture("yamato_reload_idle_quick.vnmclip_c");
    let clip = decode_nm_clip(&bytes).expect("decode");
    let orig_blob = clip.compressed_pose_data.clone();

    let mut bigger = orig_blob.clone();
    bigger.extend(std::iter::repeat_n(0xAB, 300)); // +300 bytes, still < 16 KB
    assert!(bigger.len() <= 16384);

    let patched =
        morphic::patch_kv3_resource_sole_blob(&bytes, &bigger).expect("write a longer sole blob");
    let redec = decode_nm_clip(&patched).expect("re-decode after resize");

    assert_eq!(
        redec.compressed_pose_data, bigger,
        "blob round-trips at the new length"
    );
    assert_eq!(redec.frame_count, clip.frame_count);
    // The pose tracks are unchanged (offsets untouched, the extra bytes unread).
    assert_eq!(
        redec.tracks, clip.tracks,
        "tracks unchanged by trailing bytes"
    );
}

#[test]
fn reencode_adds_a_rotation_channel() {
    // The encoder step toward authoring: animate a bone whose rotation was static
    // in the slot (the common Blender case), at a fixed frame count. The pose
    // stream grows by 3 u16/frame, the offsets shift, and the bone's
    // m_bIsRotationStatic flips to false. Re-decode must show the new animated
    // rotation and leave every other track unchanged.
    let bytes = fixture("yamato_reload_idle_quick.vnmclip_c");
    let clip = decode_nm_clip(&bytes).expect("decode");
    let frames = clip.frame_count as usize;

    // Pick a track that is rotation-static (its rotations are None).
    let target = clip
        .tracks
        .iter()
        .position(|t| t.rotations.is_none())
        .expect("clip has a static-rotation track");

    // Give it an animated rotation: yaw ramping 0 -> 45 degrees over the clip.
    let mut edited = clip.clone();
    #[allow(clippy::cast_precision_loss)]
    let rots: Vec<Quat> = (0..frames)
        .map(|f| {
            let frac = if frames > 1 {
                f as f32 / (frames - 1) as f32
            } else {
                0.0
            };
            let half = (45.0_f32.to_radians() * frac) * 0.5;
            Quat {
                x: 0.0,
                y: 0.0,
                z: half.sin(),
                w: half.cos(),
            }
        })
        .collect();
    edited.tracks[target].rotations = Some(rots.clone());

    let out =
        morphic::model::reencode_nm_clip(&bytes, &edited).expect("reencode with added channel");
    let redec = decode_nm_clip(&out).expect("re-decode reencoded clip");

    assert_eq!(redec.frame_count, clip.frame_count);
    let got = redec.tracks[target]
        .rotations
        .as_ref()
        .expect("target track is now animated");
    assert_eq!(got.len(), frames);
    // Last frame should be ~45 deg about Z (within quantization), distinct from the
    // identity at frame 0.
    let last = got[frames - 1];
    assert!(
        last.z.abs() > 0.2,
        "added rotation should reach a clear angle, got {last:?}"
    );
    assert!((got[0].z).abs() < 0.05, "frame 0 should be ~identity");

    // Every other track is unchanged from the original decode.
    for (i, (a, b)) in clip.tracks.iter().zip(redec.tracks.iter()).enumerate() {
        if i != target {
            assert_eq!(a, b, "non-target track {i} changed");
        }
    }
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
