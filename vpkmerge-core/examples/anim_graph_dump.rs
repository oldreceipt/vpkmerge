//! Dump `m_resources` of animgraph entries to see whose clip paths a hero's
//! runtime graphs actually reference. Motivated by the Viscous mystery: he
//! owns only 3 compiled clips, and his vestigial loco graph references
//! `heroes_wip/viscous/clips/*` paths that do not exist in pak01.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_graph_dump -- \
//!     <pak01_dir.vpk> <graph_entry>...

use anyhow::{Context, Result};
use morphic::kv3::Value;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;

    for entry in args {
        println!("== {entry}");
        let bytes = match vpkmerge_core::read_vpk_entry(&pak, &entry) {
            Ok(b) => b,
            Err(e) => {
                println!("   read error: {e}");
                continue;
            }
        };
        let root = match morphic::decode_kv3_resource(&bytes) {
            Ok(r) => r,
            Err(e) => {
                println!("   decode error: {e}");
                continue;
            }
        };
        match root.get("m_resources") {
            Some(Value::Array(rs)) => {
                for (i, r) in rs.iter().enumerate() {
                    if let Value::String(s) = r {
                        println!("   [{i}] {s}");
                    }
                }
            }
            _ => println!("   (no m_resources)"),
        }
    }
    Ok(())
}
