//! Scan a hero's `.vnmclip_c` entries and print duration + additive flag.
//! Motivated by experiment G3: Abrams' flinch override loaded (no vanilla
//! flinch played) but showed nothing, the signature of an additive slot fed
//! a non-additive clip.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_clip_scan -- \
//!     <pak01_dir.vpk> <listing.txt> <entry_prefix>

use anyhow::{Context, Result};
use morphic::kv3::Value;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let listing = args.next().context("missing arg: path to listing file")?;
    let prefix = args.next().context("missing arg: entry prefix filter")?;

    let listing = std::fs::read_to_string(&listing)?;
    let mut additive = Vec::new();
    let mut plain = 0usize;
    let mut failed = 0usize;

    for entry in listing
        .lines()
        .filter(|l| l.starts_with(&prefix) && l.ends_with(".vnmclip_c"))
    {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            failed += 1;
            continue;
        };
        let Ok(root) = morphic::decode_kv3_resource(&bytes) else {
            failed += 1;
            continue;
        };
        let is_additive = root
            .get("m_bIsAdditive")
            .is_some_and(|v| matches!(v, Value::Bool(true)));
        let duration = match root.get("m_flDuration") {
            Some(Value::Double(d)) => *d,
            _ => f64::NAN,
        };
        if is_additive {
            additive.push(format!("{entry}  ({duration:.2}s)"));
        } else {
            plain += 1;
        }
    }

    println!("ADDITIVE clips under {prefix}:");
    for a in &additive {
        println!("  {a}");
    }
    println!(
        "summary: {} additive, {plain} non-additive, {failed} failed",
        additive.len()
    );
    Ok(())
}
