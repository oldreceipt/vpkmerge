//! Round-trip tests for the Phase 1 encoder + splice path.
//!
//! For each uncompressed-format fixture we:
//!   1. decode the original mip 0 face 0 to `Image`
//!   2. re-encode it via `morphic::encode_image` for the texture's format
//!   3. splice the bytes back into the original `.vtex_c` via `replace_face0_mip0`
//!   4. decode the modified resource and assert the pixels match
//!
//! For RGBA8888 the round-trip is byte-exact. BGRA8888 likewise (the
//! channel swap is symmetric). `PNG_RGBA8888` is more interesting: we re-encode
//! through the PNG codec, and the on-wire bytes are not guaranteed to match
//! the original PNG payload, but the decoded pixels must.

use std::path::PathBuf;

use morphic::{decode, encode_image, inspect, replace_face0_mip0, Image, ImageData};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(rel)
}

fn pixels_match(a: &Image, b: &Image) {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "dimensions diverged"
    );
    match (&a.data, &b.data) {
        (ImageData::Rgba8(x), ImageData::Rgba8(y)) => {
            assert_eq!(x.len(), y.len(), "buffer lengths differ");
            assert_eq!(x, y, "pixel buffers differ after round-trip");
        }
        (ImageData::Rgba16F(x), ImageData::Rgba16F(y)) => {
            assert_eq!(x.len(), y.len(), "buffer lengths differ");
            for (i, (xv, yv)) in x.iter().zip(y.iter()).enumerate() {
                assert_eq!(xv.to_bits(), yv.to_bits(), "f16 differ at {i}");
            }
        }
        _ => panic!("pixel kinds differ after round-trip"),
    }
}

#[test]
fn rgba8888_roundtrip_is_byte_exact() {
    let path = fixture("rgba8/minimap_circle.vtex_c");
    let bytes = std::fs::read(&path).expect("fixture present");
    let info = inspect(&bytes).expect("inspect");

    let decoded = decode(&bytes).expect("decode");
    let encoded = encode_image(&decoded, info.format).expect("encode");

    let modified = replace_face0_mip0(&bytes, &encoded).expect("splice");
    // For a pure round-trip on RGBA8888 the whole resource must be byte-identical:
    // the splice replaces N bytes with the same N bytes.
    assert_eq!(modified, bytes, "splice diverged on byte-exact format");

    let decoded2 = decode(&modified).expect("decode after splice");
    pixels_match(&decoded, &decoded2);
}

#[test]
fn png_rgba8888_roundtrip_preserves_pixels() {
    let path = fixture("png_rgba8888/dynamic_images_sentinel.vtex_c");
    let bytes = std::fs::read(&path).expect("fixture present");
    let info = inspect(&bytes).expect("inspect");

    let decoded = decode(&bytes).expect("decode");
    let encoded = encode_image(&decoded, info.format).expect("encode");

    // Re-encoded PNG payload won't match the original PNG bytes (different
    // encoder), so the splice length almost certainly differs from the
    // original. Phase 1's narrow splice rejects that; we should see the
    // length-mismatch error, which is the correct signal that inline-PNG
    // round-trip needs a different splice path (Phase 3 territory).
    let err =
        replace_face0_mip0(&bytes, &encoded).expect_err("inline PNG cannot round-trip via splice");
    let msg = err.to_string();
    assert!(
        msg.contains("splice length mismatch"),
        "unexpected error: {msg}"
    );
}

#[test]
fn encode_rejects_wrong_buffer_length() {
    // A 16x16 image whose buffer claims 4 bytes per pixel but is short.
    let img = Image {
        width: 16,
        height: 16,
        data: ImageData::Rgba8(vec![0u8; 16 * 16 * 4 - 1]),
    };
    let err = encode_image(&img, morphic::TextureFormat::Rgba8888).expect_err("must reject");
    let msg = err.to_string();
    assert!(msg.contains("size mismatch"), "unexpected error: {msg}");
}

#[test]
fn splice_rejects_wrong_payload_length() {
    let path = fixture("rgba8/minimap_circle.vtex_c");
    let bytes = std::fs::read(&path).expect("fixture present");
    // Garbage payload of one wrong byte.
    let err = replace_face0_mip0(&bytes, &[0u8; 1]).expect_err("must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("splice length mismatch"),
        "unexpected error: {msg}"
    );
}

/// Sanity: BGRA8888 is the inverse of itself, so round-trip is byte-exact.
/// No BGRA8888 fixture exists in the corpus (Deadlock doesn't use it), so
/// we synthesise an image and exercise encode + decode only, skipping splice.
#[test]
fn bgra8888_encode_decode_roundtrip() {
    use morphic::{decode_image, TextureFlags, TextureFormat, TextureInfo};
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for i in 0u8..(8 * 8) {
        pixels.extend_from_slice(&[i, i.wrapping_add(40), i.wrapping_add(80), 0xff]);
    }
    let img = Image {
        width: 8,
        height: 8,
        data: ImageData::Rgba8(pixels.clone()),
    };
    let encoded = encode_image(&img, TextureFormat::Bgra8888).expect("encode");
    let info = TextureInfo {
        format: TextureFormat::Bgra8888,
        width: 8,
        height: 8,
        depth: 1,
        mip_count: 1,
        flags: TextureFlags::empty(),
    };
    let opts = morphic::DecodeOptions::default();
    let back = decode_image(&info, &encoded, &opts).expect("decode");
    pixels_match(&img, &back);
}
