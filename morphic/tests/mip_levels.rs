//! Mip-level coverage (M9 second half).
//!
//! Decodes all mip levels of the committed 128x128 BC6H sky cubemap and
//! asserts that dimensions halve correctly, every decode returns a
//! distinct buffer, and out-of-range mips are rejected. Exercises the
//! mip-slicing arithmetic in `texture::pixel_data` plus the scratch-buffer
//! sub-4 block path in `bcn::decode_bc6h` (the smallest mip is 4x4 which
//! is exactly one block; if the texture had a `mip_count` of 7 we'd hit
//! 2x2 which is a quarter of a block).

use std::path::PathBuf;

use morphic::{decode_at, inspect, DecodeError, DecodeOptions, ImageData};
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
fn all_mips_decode_with_halving_dims_and_distinct_buffers() {
    let bytes = std::fs::read(fixture_path()).expect("fixture present");
    let info = inspect(&bytes).expect("inspect");
    assert!(info.mip_count >= 2, "fixture needs at least 2 mips");

    let mut hashes = Vec::with_capacity(usize::from(info.mip_count));
    for mip in 0..info.mip_count {
        let img = decode_at(
            &bytes,
            &DecodeOptions {
                mip,
                slice: 0,
                face: 0,
            },
        )
        .unwrap_or_else(|e| panic!("decode mip {mip}: {e}"));
        let expected_w = (u32::from(info.width) >> u32::from(mip)).max(1);
        let expected_h = (u32::from(info.height) >> u32::from(mip)).max(1);
        assert_eq!(
            (img.width, img.height),
            (expected_w, expected_h),
            "mip {mip}"
        );
        let ImageData::Rgba16F(p) = &img.data else {
            panic!("mip {mip}: expected Rgba16F");
        };
        assert_eq!(
            p.len(),
            (expected_w * expected_h * 4) as usize,
            "mip {mip} buffer length"
        );
        hashes.push(sha_of_f16(p));
    }
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i], hashes[j],
                "mips {i} and {j} produced identical buffers"
            );
        }
    }
}

#[test]
fn out_of_range_mip_is_rejected() {
    let bytes = std::fs::read(fixture_path()).expect("fixture present");
    let info = inspect(&bytes).expect("inspect");
    let err = decode_at(
        &bytes,
        &DecodeOptions {
            mip: info.mip_count,
            slice: 0,
            face: 0,
        },
    )
    .expect_err("mip == mip_count must be out of range");
    assert!(
        matches!(err, DecodeError::InvalidTarget { .. }),
        "got {err:?}"
    );
}
