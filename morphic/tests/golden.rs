//! Golden-output test harness.
//!
//! Walks `MORPHIC_FIXTURES` (defaults to `fixtures/`) for `*.vtex_c`. For
//! each, loads sibling `.png` + `.meta.json` and compares morphic's decode
//! against the oracle's output under the tolerance declared in the meta.
//!
//! Failures are accumulated and reported at the end so cargo's output shows
//! the full list of broken fixtures in one run, not just the first.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use morphic::{decode, inspect, DecodeError, ImageData, TextureFormat};

#[derive(Debug, Deserialize)]
struct Meta {
    format: String,
    width: u16,
    height: u16,
    /// VRF crops the rendered PNG to these dims via `NonPow2` metadata.
    /// When `actual_width < width`, the Rust test crops morphic's decode to
    /// the upper-left corner before diffing.
    #[serde(default)]
    actual_width: u16,
    #[serde(default)]
    actual_height: u16,
    #[allow(dead_code)]
    depth: u16,
    mip_count: u8,
    #[allow(dead_code)]
    flags: Vec<String>,
    source_sha256: String,
    #[allow(dead_code)]
    vrf_version: String,
    tolerance: Tolerance,
}

#[derive(Debug, Deserialize)]
struct Tolerance {
    kind: String,
    #[serde(default)]
    epsilon: Option<f64>,
    #[serde(default)]
    abs: Option<f64>,
    #[serde(default)]
    rel: Option<f64>,
}

fn fixtures_dir() -> PathBuf {
    let env = std::env::var("MORPHIC_FIXTURES").unwrap_or_else(|_| "fixtures".to_string());
    PathBuf::from(env)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(s, "{b:02x}").expect("write to String never fails");
    }
    s
}

enum Outcome {
    Pass,
    /// Decoder for this format hasn't landed yet. Counted separately from a
    /// real failure so CI stays green while the milestone list works through
    /// formats, while still surfacing each pending fixture in the output.
    Pending(String),
    /// Something a decoder change actually broke. Fails the build.
    Fail(String),
}

#[test]
fn goldens() {
    let root = fixtures_dir();
    assert!(
        root.exists(),
        "fixtures dir not found: {} (set MORPHIC_FIXTURES?)",
        root.display()
    );

    let mut passed = 0usize;
    let mut pending: BTreeMap<String, String> = BTreeMap::new();
    let mut failed: BTreeMap<String, String> = BTreeMap::new();

    for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("vtex_c") {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        match check_one(path) {
            Outcome::Pass => passed += 1,
            Outcome::Pending(msg) => {
                pending.insert(rel, msg);
            }
            Outcome::Fail(msg) => {
                failed.insert(rel, msg);
            }
        }
    }

    let total = passed + pending.len() + failed.len();
    assert!(
        total > 0,
        "no .vtex_c fixtures found under {}",
        root.display()
    );

    // Always print the full breakdown so the dev sees the wall of progress.
    let mut report = format!(
        "\n{passed} passed, {} pending, {} failed (of {total})\n",
        pending.len(),
        failed.len()
    );
    for (k, v) in &pending {
        writeln!(report, "  [PENDING] {k}: {v}").expect("write to String never fails");
    }
    for (k, v) in &failed {
        writeln!(report, "  [FAIL]    {k}: {v}").expect("write to String never fails");
    }
    println!("{report}");

    assert!(failed.is_empty(), "{}", report);
}

fn check_one(vtex_path: &Path) -> Outcome {
    match run_one(vtex_path) {
        Ok(()) => Outcome::Pass,
        Err(msg) if msg.starts_with("[stub]") => Outcome::Pending(msg),
        Err(msg) => Outcome::Fail(msg),
    }
}

fn run_one(vtex_path: &Path) -> Result<(), String> {
    let meta_path = vtex_path.with_extension("meta.json");
    let png_path = vtex_path.with_extension("png");

    let vtex_bytes = std::fs::read(vtex_path).map_err(|e| format!("read vtex_c: {e}"))?;
    let meta_bytes =
        std::fs::read(&meta_path).map_err(|e| format!("meta missing ({e}); run 'just goldens'"))?;
    let meta: Meta = serde_json::from_slice(&meta_bytes).map_err(|e| format!("meta parse: {e}"))?;

    // Staleness guard.
    let actual = sha256_hex(&vtex_bytes);
    if actual != meta.source_sha256 {
        return Err(format!(
            "stale golden: vtex_c sha256 is {actual}, meta says {} (regenerate with 'just goldens')",
            meta.source_sha256
        ));
    }

    // Header check.
    let info = inspect(&vtex_bytes).map_err(|e| format!("inspect: {e}"))?;
    let expected_format = TextureFormat::from_meta_name(&meta.format)
        .map_err(|e| format!("unknown format name {}: {e}", meta.format))?;
    if info.format != expected_format {
        return Err(format!(
            "format mismatch: morphic={:?}, meta={:?}",
            info.format, expected_format
        ));
    }
    if info.width != meta.width || info.height != meta.height || info.mip_count != meta.mip_count {
        return Err(format!(
            "dims mismatch: morphic={}x{} mips={}, meta={}x{} mips={}",
            info.width, info.height, info.mip_count, meta.width, meta.height, meta.mip_count
        ));
    }

    // Decode + compare.
    let img = match decode(&vtex_bytes) {
        Ok(img) => img,
        Err(DecodeError::Unimplemented(fmt)) => {
            return Err(format!("[stub] {fmt:?} decoder not yet implemented"));
        }
        Err(e) => return Err(format!("decode: {e}")),
    };

    let png_bytes =
        std::fs::read(&png_path).map_err(|e| format!("png missing ({e}); run 'just goldens'"))?;
    let expected = image::load_from_memory(&png_bytes)
        .map_err(|e| format!("png decode: {e}"))?
        .to_rgba8();

    // VRF crops to ActualWidth/Height when NonPow2 metadata is present. If
    // the meta is from an older oracle without those fields, fall back to
    // header dims.
    let cmp_w = if meta.actual_width == 0 {
        meta.width
    } else {
        meta.actual_width
    };
    let cmp_h = if meta.actual_height == 0 {
        meta.height
    } else {
        meta.actual_height
    };
    if u32::from(cmp_w) != expected.width() || u32::from(cmp_h) != expected.height() {
        return Err(format!(
            "png dims mismatch: png={}x{}, meta actual={}x{}",
            expected.width(),
            expected.height(),
            cmp_w,
            cmp_h
        ));
    }

    // HDR path: decoder gave f16 output. Diff against the .f32 sibling the
    // oracle emits for IsHighDynamicRange textures. If the sibling isn't
    // there yet (older fixture set), fall back to PENDING.
    if let ImageData::Rgba16F(pixels) = &img.data {
        let f32_path = vtex_path.with_extension("f32");
        if !f32_path.exists() {
            return Err(
                "[stub] HDR (Rgba16F) oracle sibling .f32 missing; run 'just goldens-force'"
                    .to_string(),
            );
        }
        let f32_bytes = std::fs::read(&f32_path).map_err(|e| format!("read f32: {e}"))?;
        return diff_rgba16f(
            pixels,
            meta.width,
            cmp_w,
            cmp_h,
            &f32_bytes,
            &meta.tolerance,
        );
    }

    let ImageData::Rgba8(actual_rgba_full) = &img.data else {
        unreachable!("Rgba16F handled above");
    };
    let cropped = crop_rgba8(actual_rgba_full, meta.width, cmp_w, cmp_h);

    diff_rgba8(&cropped, expected.as_raw(), &meta.tolerance)
}

/// HDR diff: take the f16 decoder output, crop to ActualWidth/Height (matching
/// what the oracle's bitmap dimensions are), promote to f32, and compare
/// against the raw `RgbaF32` bytes the oracle dumped. Per-channel pass condition
/// is `|a - e| <= abs OR |a - e| <= rel * |e|`, which lets small values be
/// gated by `abs` and large values by `rel`.
fn diff_rgba16f(
    decoded: &[half::f16],
    stride_px: u16,
    cmp_w: u16,
    cmp_h: u16,
    expected_f32_bytes: &[u8],
    tol: &Tolerance,
) -> Result<(), String> {
    if tol.kind != "hdr_eps" {
        return Err(format!(
            "tolerance kind {:?} does not apply to HDR (expected hdr_eps)",
            tol.kind
        ));
    }
    let abs = tol.abs.unwrap_or(0.000_977);
    let rel = tol.rel.unwrap_or(0.005);

    let expected_floats = expected_f32_bytes.len() / 4;
    let expected_pixels = expected_floats / 4;
    let needed_pixels = usize::from(cmp_w) * usize::from(cmp_h);
    if expected_pixels != needed_pixels {
        return Err(format!(
            "f32 sibling pixel count mismatch: file={expected_pixels}, meta actual={needed_pixels} ({cmp_w}x{cmp_h})"
        ));
    }

    let stride = usize::from(stride_px);
    let mut first_fail: Option<(usize, usize, f32, f32, f32)> = None;
    let mut count = 0usize;
    for y in 0..usize::from(cmp_h) {
        for x in 0..usize::from(cmp_w) {
            for c in 0..4 {
                let a = decoded[(y * stride + x) * 4 + c].to_f32();
                let cmp_idx = (y * usize::from(cmp_w) + x) * 4 + c;
                let e_bytes: [u8; 4] = expected_f32_bytes[cmp_idx * 4..cmp_idx * 4 + 4]
                    .try_into()
                    .unwrap();
                let e = f32::from_le_bytes(e_bytes);
                let diff = (a - e).abs();
                #[allow(clippy::cast_possible_truncation)]
                let abs_f32 = abs as f32;
                #[allow(clippy::cast_possible_truncation)]
                let rel_f32 = rel as f32;
                if !(diff <= abs_f32 || diff <= rel_f32 * e.abs()) && first_fail.is_none() {
                    first_fail = Some((x, y, a, e, diff));
                }
                count += 1;
            }
        }
    }

    if let Some((x, y, a, e, diff)) = first_fail {
        Err(format!(
            "hdr_eps mismatch at ({x},{y}): actual={a}, expected={e}, |diff|={diff} (abs={abs}, rel={rel}, {count} channels checked)"
        ))
    } else {
        Ok(())
    }
}

/// Take the upper-left `w x h` region of an RGBA8 buffer that's `stride`
/// pixels wide. Returns the contiguous cropped buffer (`w * h * 4` bytes).
fn crop_rgba8(full: &[u8], stride_px: u16, w: u16, h: u16) -> Vec<u8> {
    let stride_bytes = usize::from(stride_px) * 4;
    let row_bytes = usize::from(w) * 4;
    let mut out = Vec::with_capacity(usize::from(h) * row_bytes);
    for y in 0..usize::from(h) {
        let start = y * stride_bytes;
        out.extend_from_slice(&full[start..start + row_bytes]);
    }
    out
}

fn diff_rgba8(actual: &[u8], expected: &[u8], tol: &Tolerance) -> Result<(), String> {
    if actual.len() != expected.len() {
        return Err(format!(
            "rgba length mismatch: actual={}, expected={}",
            actual.len(),
            expected.len()
        ));
    }
    match tol.kind.as_str() {
        "byte_exact" => {
            if actual == expected {
                Ok(())
            } else {
                let mismatch = actual
                    .iter()
                    .zip(expected.iter())
                    .enumerate()
                    .find(|(_, (a, e))| a != e);
                let (idx, a, e) = match mismatch {
                    Some((i, (a, e))) => (i, *a, *e),
                    None => (0, 0, 0),
                };
                Err(format!(
                    "byte_exact mismatch first at byte {idx}: actual={a:#04x}, expected={e:#04x}"
                ))
            }
        }
        "mae_u8" => {
            let eps = tol.epsilon.unwrap_or(2.0);
            let sum: u64 = actual
                .iter()
                .zip(expected.iter())
                .map(|(a, e)| u64::from(a.abs_diff(*e)))
                .sum();
            // f64 mantissa is 52 bits; sum/len fit comfortably for any
            // reasonable fixture (a 4096^2 image yields sum < 2^36).
            #[allow(clippy::cast_precision_loss)]
            let mae = sum as f64 / actual.len() as f64;
            if mae <= eps {
                Ok(())
            } else {
                Err(format!("mae_u8 {mae:.3} exceeds eps {eps}"))
            }
        }
        other => Err(format!("unsupported tolerance kind: {other}")),
    }
}
