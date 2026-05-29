//! M2 validation: decode each committed `*.meshopt` raw block (a real MVTX /
//! MIDX payload sliced from `hornet.vmdl_c`) and assert the result matches the
//! oracle golden in the sibling `*.meshopt.json` byte-for-byte (length +
//! SHA-256). Goldens come from `tools/morphic-oracle mesh-buffers`, which
//! decodes via `ValveResourceFormat`'s own meshopt path.

use std::path::PathBuf;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{decode_index_buffer, decode_vertex_buffer, encode_index_buffer, encode_vertex_buffer};

#[derive(Deserialize)]
struct Golden {
    kind: String,
    element_count: usize,
    element_size: usize,
    meshopt: bool,
    zstd: bool,
    decoded_len: usize,
    sha256: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/meshopt")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        write!(s, "{b:02x}").expect("write to String never fails");
    }
    s
}

#[test]
fn meshopt_buffers_match_oracle() {
    let dir = fixtures_dir();
    let mut checked = 0usize;

    for entry in std::fs::read_dir(&dir).expect("meshopt fixtures dir present") {
        let path = entry.expect("dir entry").path();
        // Drive off the .meshopt raw blocks; their sibling .meshopt.json is the golden.
        if path.extension().and_then(|e| e.to_str()) != Some("meshopt") {
            continue;
        }

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read(&path).expect("read .meshopt");
        let golden_path = path.with_extension("meshopt.json");
        let golden: Golden =
            serde_json::from_str(&std::fs::read_to_string(&golden_path).expect("read golden"))
                .expect("parse golden");

        assert!(golden.meshopt, "{name}: fixture is not meshopt-compressed");
        assert!(!golden.zstd, "{name}: zstd buffers are not supported");

        let decoded = match golden.kind.as_str() {
            "vertex" => decode_vertex_buffer(golden.element_count, golden.element_size, &raw)
                .unwrap_or_else(|e| panic!("{name}: vertex decode failed: {e}")),
            "index" => decode_index_buffer(golden.element_count, golden.element_size, &raw)
                .unwrap_or_else(|e| panic!("{name}: index decode failed: {e}")),
            other => panic!("{name}: unknown buffer kind {other:?}"),
        };

        assert_eq!(
            decoded.len(),
            golden.decoded_len,
            "{name}: decoded length mismatch"
        );
        assert_eq!(
            sha256_hex(&decoded),
            golden.sha256,
            "{name}: decoded bytes differ from oracle"
        );
        checked += 1;
    }

    assert!(
        checked >= 5,
        "expected to check the committed meshopt fixtures, got {checked}"
    );
}

/// The encoder is the basis for vertex-displacement model edits (Tier 0): the
/// spike gate is that `decode(encode(x)) == x` on real Source 2 vertex streams,
/// so a re-encoded MVTX block reads back identically (and the engine, using the
/// same meshopt codec, loads it). We re-encode each committed vertex fixture's
/// decoded stream and assert it decodes back to the oracle golden byte-for-byte.
#[test]
fn vertex_encode_round_trips_through_decoder() {
    let dir = fixtures_dir();
    let mut checked = 0usize;

    for entry in std::fs::read_dir(&dir).expect("meshopt fixtures dir present") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("meshopt") {
            continue;
        }
        let golden_path = path.with_extension("meshopt.json");
        let golden: Golden =
            serde_json::from_str(&std::fs::read_to_string(&golden_path).expect("read golden"))
                .expect("parse golden");
        if golden.kind != "vertex" {
            continue;
        }

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read(&path).expect("read .meshopt");

        // Original decoded interleaved stream (already validated == oracle elsewhere).
        let decoded = decode_vertex_buffer(golden.element_count, golden.element_size, &raw)
            .unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));

        // Re-encode it, then decode the re-encoded buffer.
        let reencoded = encode_vertex_buffer(golden.element_count, golden.element_size, &decoded)
            .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
        assert_eq!(reencoded[0], 0xa1, "{name}: expected codec v1 header");

        let redecoded = decode_vertex_buffer(golden.element_count, golden.element_size, &reencoded)
            .unwrap_or_else(|e| panic!("{name}: re-decode failed: {e}"));

        assert_eq!(
            redecoded.len(),
            golden.decoded_len,
            "{name}: re-decoded length mismatch"
        );
        assert_eq!(
            redecoded, decoded,
            "{name}: re-encode/re-decode did not round-trip"
        );
        assert_eq!(
            sha256_hex(&redecoded),
            golden.sha256,
            "{name}: round-tripped bytes differ from oracle"
        );
        checked += 1;
    }

    assert!(
        checked >= 3,
        "expected to round-trip the committed vertex fixtures, got {checked}"
    );
}

/// Index analog of [`vertex_encode_round_trips_through_decoder`]: the Tier 1b
/// gate. Re-encode each committed index fixture's decoded triangle list and
/// assert it decodes back to the oracle golden byte-for-byte (so a re-encoded
/// `MIDX` block reads back identically through the codec the engine also uses).
#[test]
fn index_encode_round_trips_through_decoder() {
    let dir = fixtures_dir();
    let mut checked = 0usize;

    for entry in std::fs::read_dir(&dir).expect("meshopt fixtures dir present") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("meshopt") {
            continue;
        }
        let golden_path = path.with_extension("meshopt.json");
        let golden: Golden =
            serde_json::from_str(&std::fs::read_to_string(&golden_path).expect("read golden"))
                .expect("parse golden");
        if golden.kind != "index" {
            continue;
        }

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let raw = std::fs::read(&path).expect("read .meshopt");

        // Original decoded triangle list (already validated == oracle elsewhere).
        let decoded = decode_index_buffer(golden.element_count, golden.element_size, &raw)
            .unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));

        // Re-encode it, then decode the re-encoded buffer.
        let reencoded = encode_index_buffer(golden.element_count, golden.element_size, &decoded)
            .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
        assert_eq!(reencoded[0], 0xe1, "{name}: expected index codec v1 header");

        let redecoded = decode_index_buffer(golden.element_count, golden.element_size, &reencoded)
            .unwrap_or_else(|e| panic!("{name}: re-decode failed: {e}"));

        assert_eq!(
            redecoded.len(),
            golden.decoded_len,
            "{name}: re-decoded length mismatch"
        );
        assert_eq!(
            redecoded, decoded,
            "{name}: index re-encode/re-decode did not round-trip"
        );
        assert_eq!(
            sha256_hex(&redecoded),
            golden.sha256,
            "{name}: round-tripped bytes differ from oracle"
        );
        checked += 1;
    }

    assert!(
        checked >= 2,
        "expected to round-trip the committed index fixtures, got {checked}"
    );
}

/// The committed index fixtures are all 16-bit; exercise the 32-bit (`index_size
/// == 4`) lane too with a synthetic triangle list whose indices exceed `u16` and
/// include a backwards jump (negative delta).
#[test]
fn index_encode_round_trips_u32() {
    let indices: [u32; 12] = [0, 1, 2, 2, 1, 3, 100, 200, 300, 70_000, 1, 70_001];
    let index_count = indices.len();
    let mut raw = Vec::with_capacity(index_count * 4);
    for i in &indices {
        raw.extend_from_slice(&i.to_le_bytes());
    }

    let encoded = encode_index_buffer(index_count, 4, &raw).expect("encode u32 indices");
    assert_eq!(encoded[0], 0xe1, "expected index codec v1 header");

    let decoded = decode_index_buffer(index_count, 4, &encoded).expect("decode u32 indices");
    assert_eq!(decoded, raw, "u32 index round-trip mismatch");
}

/// `zigzag8` must be the exact inverse of the decoder's `unzigzag8` across all
/// 256 byte values, since the encoder relies on that bijection.
#[test]
fn zigzag8_inverts_unzigzag8() {
    // Mirror of the decoder's private `unzigzag8`.
    fn unzigzag8(v: u8) -> u8 {
        (0u8.wrapping_sub(v & 1)) ^ (v >> 1)
    }
    fn zigzag8(d: u8) -> u8 {
        if d < 128 {
            d << 1
        } else {
            ((255 - d) << 1) | 1
        }
    }
    for d in 0u8..=255 {
        assert_eq!(unzigzag8(zigzag8(d)), d, "zigzag8 not invertible at {d}");
    }
}
