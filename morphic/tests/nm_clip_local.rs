//! Local (non-CI) stress check for the NM pose codec across a whole Deadlock
//! pak. Gated on `MORPHIC_MODEL_VPK` pointing at a `pak01_dir.vpk`; skipped
//! otherwise. Where the committed `tests/nm_clip.rs` proves byte-exact
//! round-trips on a few hand-picked fixtures, this asserts the codec's behaviour
//! over *every* animated `.vnmclip_c`:
//!  - translation and scale channels round-trip **byte-exactly** (zero drift),
//!  - rotation channels round-trip pose-identically within tight tolerance
//!    (the smallest-three packing's largest-component tie can pick an equivalent
//!    encoding, never more than ~0.01 rad off),
//!  - and the great majority re-encode byte-for-byte.
//!
//! Run with: `MORPHIC_MODEL_VPK=/path/to/pak01_dir.vpk cargo test -p morphic
//! --test nm_clip_local -- --nocapture` (part of the `just` daily loop).

use morphic::model::{decode_nm_clip, decode_pose_stream, encode_compressed_pose, NmTrack};

/// Worst per-channel delta between two decodes: rotation as the geodesic angle
/// (radians, sign-agnostic), translation/scale as absolute differences.
fn max_delta(a: &[NmTrack], b: &[NmTrack]) -> (f32, f32, f32) {
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
                tr = tr
                    .max((pa.x - pb.x).abs())
                    .max((pa.y - pb.y).abs())
                    .max((pa.z - pb.z).abs());
            }
        }
        if let (Some(va), Some(vb)) = (&ta.scales, &tb.scales) {
            for (x, y) in va.iter().zip(vb.iter()) {
                sc = sc.max((x - y).abs());
            }
        }
    }
    (rot, tr, sc)
}

#[test]
fn nm_pose_codec_round_trips_every_clip() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local NM pose-codec check");
        return;
    };
    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut paths: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vnmclip_c"))
        .cloned()
        .collect();
    paths.sort();

    let (mut checked, mut byte_exact) = (0usize, 0usize);
    let (mut worst_rot, mut worst_tr, mut worst_sc) = (0f32, 0f32, 0f32);
    let mut worst_clip = String::new();

    for p in &paths {
        let Ok(bytes) = vpk.get_file(p).and_then(|mut f| f.read_all()) else {
            continue;
        };
        let Ok(clip) = decode_nm_clip(&bytes) else {
            continue;
        };
        if clip.compressed_pose_data.is_empty() {
            continue; // static clip, no stream to exercise
        }
        checked += 1;

        let (data2, offsets2) = encode_compressed_pose(&clip);
        assert_eq!(
            offsets2, clip.compressed_pose_offsets,
            "{p}: frame offsets changed on re-encode"
        );
        if data2 == clip.compressed_pose_data {
            byte_exact += 1;
        }

        let settings: Vec<_> = clip.tracks.iter().map(|t| t.settings).collect();
        let tracks2 = decode_pose_stream(&settings, &data2, &offsets2, clip.frame_count)
            .unwrap_or_else(|e| panic!("{p}: re-decode failed: {e}"));
        let (rot, tr, sc) = max_delta(&clip.tracks, &tracks2);
        if rot > worst_rot {
            worst_rot = rot;
            worst_clip = p.clone();
        }
        worst_tr = worst_tr.max(tr);
        worst_sc = worst_sc.max(sc);

        // Translation and scale must be exact; rotation within tight tolerance.
        assert!(tr == 0.0, "{p}: translation drift {tr} on round-trip");
        assert!(sc == 0.0, "{p}: scale drift {sc} on round-trip");
        assert!(
            rot < 0.01,
            "{p}: rotation drift {rot} rad exceeds tolerance"
        );
    }

    assert!(
        checked > 100,
        "expected many animated clips, found {checked}"
    );
    #[allow(clippy::cast_precision_loss)]
    let pct = byte_exact as f64 / checked as f64 * 100.0;
    eprintln!(
        "NM pose codec: {checked} animated clips, {byte_exact} byte-exact ({pct:.1}%); \
         worst rotation drift {worst_rot:.5} rad on {worst_clip}, \
         translation {worst_tr}, scale {worst_sc}"
    );
    // The overwhelming majority must re-encode byte-for-byte; the rest differ
    // only by the equivalent-encoding tie in the smallest-three quaternion.
    assert!(pct > 80.0, "byte-exact rate {pct:.1}% lower than expected");
}
