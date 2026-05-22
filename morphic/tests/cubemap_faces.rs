//! Face-selection coverage for cubemap textures (M10 first half).
//!
//! Decodes the committed 128x128 BC6H sky cubemap fixture at every face
//! 0..5 and asserts each face produces a distinct decoded buffer. This
//! catches off-by-one errors in the face-slicing arithmetic in
//! `texture::pixel_data` without needing per-face oracle goldens.

use std::path::PathBuf;

use morphic::{decode_at, DecodeError, DecodeOptions, ImageData};
use sha2::{Digest, Sha256};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/bc6h/sky_l4d_c1_2_hdr_cube_pfm_b562e1cd.vtex_c")
}

fn sha_of_f16(buf: &[half::f16]) -> [u8; 32] {
    let mut h = Sha256::new();
    for v in buf {
        h.update(v.to_bits().to_le_bytes());
    }
    h.finalize().into()
}

#[test]
fn all_six_faces_decode_to_distinct_buffers() {
    let bytes = std::fs::read(fixture_path()).expect("fixture present");
    let mut hashes = [[0u8; 32]; 6];
    for face in 0u8..6 {
        let img = decode_at(
            &bytes,
            &DecodeOptions {
                mip: 0,
                slice: 0,
                face,
            },
        )
        .unwrap_or_else(|e| panic!("decode face {face}: {e}"));
        assert_eq!((img.width, img.height), (128, 128), "face {face}");
        let ImageData::Rgba16F(p) = &img.data else {
            panic!("face {face}: expected Rgba16F");
        };
        assert_eq!(p.len(), 128 * 128 * 4, "face {face}");
        hashes[usize::from(face)] = sha_of_f16(p);
    }
    for i in 0..6 {
        for j in (i + 1)..6 {
            assert_ne!(
                hashes[i], hashes[j],
                "faces {i} and {j} decoded to identical buffers (slicing bug?)"
            );
        }
    }
}

#[test]
fn face_six_is_rejected() {
    let bytes = std::fs::read(fixture_path()).expect("fixture present");
    let err = decode_at(
        &bytes,
        &DecodeOptions {
            mip: 0,
            slice: 0,
            face: 6,
        },
    )
    .expect_err("face=6 must be out of range for a 6-face cubemap");
    assert!(
        matches!(err, DecodeError::InvalidTarget { face: 6, .. }),
        "expected InvalidTarget {{ face: 6, .. }}, got {err:?}"
    );
}

#[test]
fn non_cubemap_rejects_nonzero_face() {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/bc7/generic_sleep_icon.vtex_c");
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    let err = decode_at(
        &bytes,
        &DecodeOptions {
            mip: 0,
            slice: 0,
            face: 1,
        },
    )
    .expect_err("face != 0 must be invalid for non-cubemap textures");
    assert!(
        matches!(err, DecodeError::InvalidTarget { face: 1, .. }),
        "got {err:?}"
    );
}
