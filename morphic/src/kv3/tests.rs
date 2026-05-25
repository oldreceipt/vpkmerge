//! M1 validation: parse the committed raw KV3 blocks and compare the resulting
//! [`Value`] tree against the oracle's canonical JSON dump (produced by
//! `tools/morphic-oracle kv3-dump`, which wraps `ValveResourceFormat`).
//!
//! The comparison is semantic, not textual: floats are matched by IEEE-754 bit
//! pattern (golden encodes them as `{"$f64":"0xHEXBITS"}`) and blobs by
//! length + SHA-256 (`{"$bin":{"len":N,"sha256":"..."}}`), so there is no
//! float-formatting ambiguity between the C# and Rust sides. Objects are
//! matched by key (order-insensitive).

use std::path::PathBuf;

use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use super::{parse, Value};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/kv3")
        .join(name)
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

fn json_int(j: &Json) -> Option<i128> {
    j.as_i64()
        .map(i128::from)
        .or_else(|| j.as_u64().map(i128::from))
}

fn short(j: &Json) -> String {
    j.to_string().chars().take(80).collect()
}

/// Recursively asserts `value` matches the oracle `golden`, returning a path-
/// qualified message on the first divergence.
fn compare(value: &Value, golden: &Json, path: &str) -> Result<(), String> {
    match value {
        Value::Null => match golden {
            Json::Null => Ok(()),
            _ => Err(format!("{path}: expected null, golden {}", short(golden))),
        },
        Value::Bool(b) => match golden.as_bool() {
            Some(g) if g == *b => Ok(()),
            _ => Err(format!(
                "{path}: expected bool {b}, golden {}",
                short(golden)
            )),
        },
        Value::Int(i) => match json_int(golden) {
            Some(g) if g == i128::from(*i) => Ok(()),
            _ => Err(format!(
                "{path}: expected int {i}, golden {}",
                short(golden)
            )),
        },
        Value::UInt(u) => match json_int(golden) {
            Some(g) if g == i128::from(*u) => Ok(()),
            _ => Err(format!(
                "{path}: expected uint {u}, golden {}",
                short(golden)
            )),
        },
        Value::Double(d) => {
            let hex = golden
                .get("$f64")
                .and_then(Json::as_str)
                .ok_or_else(|| format!("{path}: expected $f64 marker, golden {}", short(golden)))?;
            let bits = u64::from_str_radix(hex.trim_start_matches("0x"), 16)
                .map_err(|_| format!("{path}: bad $f64 hex {hex:?}"))?;
            if d.to_bits() == bits {
                Ok(())
            } else {
                Err(format!(
                    "{path}: double bits {:#018x} != golden {:#018x}",
                    d.to_bits(),
                    bits
                ))
            }
        }
        Value::String(s) => match golden.as_str() {
            Some(g) if g == s => Ok(()),
            _ => Err(format!("{path}: string mismatch, golden {}", short(golden))),
        },
        Value::Binary(bytes) => {
            let bin = golden
                .get("$bin")
                .ok_or_else(|| format!("{path}: expected $bin marker, golden {}", short(golden)))?;
            let len = bin.get("len").and_then(Json::as_u64).unwrap_or(u64::MAX);
            let sha = bin.get("sha256").and_then(Json::as_str).unwrap_or("");
            let actual_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            if actual_len == len && sha256_hex(bytes) == sha {
                Ok(())
            } else {
                Err(format!(
                    "{path}: binary blob mismatch (len {})",
                    bytes.len()
                ))
            }
        }
        Value::Array(items) => {
            let arr = golden
                .as_array()
                .ok_or_else(|| format!("{path}: expected array, golden {}", short(golden)))?;
            if items.len() != arr.len() {
                return Err(format!(
                    "{path}: array len {} != golden {}",
                    items.len(),
                    arr.len()
                ));
            }
            for (i, (v, g)) in items.iter().zip(arr).enumerate() {
                compare(v, g, &format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        Value::Object(map) => {
            let obj = golden
                .as_object()
                .ok_or_else(|| format!("{path}: expected object, golden {}", short(golden)))?;
            if map.len() != obj.len() {
                return Err(format!(
                    "{path}: key count {} != golden {} (rust keys {:?})",
                    map.len(),
                    obj.len(),
                    map.keys().take(20).collect::<Vec<_>>()
                ));
            }
            for (k, v) in map {
                let g = obj
                    .get(k)
                    .ok_or_else(|| format!("{path}: golden missing key {k:?}"))?;
                compare(v, g, &format!("{path}.{k}"))?;
            }
            Ok(())
        }
    }
}

fn check(stem: &str) {
    let bin = std::fs::read(fixture(&format!("{stem}.kv3bin"))).expect("kv3bin fixture present");
    let golden_text =
        std::fs::read_to_string(fixture(&format!("{stem}.kv3.json"))).expect("kv3 golden present");
    let golden: Json = serde_json::from_str(&golden_text).expect("kv3 golden parses");

    let value = parse(&bin).unwrap_or_else(|e| panic!("{stem}: kv3 parse failed: {e}"));

    if let Err(msg) = compare(&value, &golden, "$") {
        panic!("{stem}: {msg}");
    }
}

#[test]
fn hornet_data_matches_oracle() {
    check("hornet_data");
}

#[test]
fn hornet_mdat0_matches_oracle() {
    check("hornet_mdat0");
}
