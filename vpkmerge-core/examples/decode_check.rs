//! Quick check that `morphic::decode_kv3_resource` succeeds on an entry (used to
//! confirm the v4 binary-blob reader unlocks the `.vpost_c` color-grade files).
//! Usage: cargo run --release -p vpkmerge-core --example decode_check -- <pak> <entry>

use anyhow::{Context, Result};
use morphic::kv3::Value;

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().context("pak")?;
    let entry = a.next().context("entry")?;
    let bytes = vpkmerge_core::read_vpk_entry(&pak, &entry)?;
    match morphic::decode_kv3_resource(&bytes) {
        Ok(v) => {
            let keys: Vec<&str> = v
                .as_object()
                .map(|o| o.iter().map(|(k, _)| k.as_str()).take(8).collect())
                .unwrap_or_default();
            println!(
                "OK: decoded {entry} ({} bytes); top keys: {keys:?}",
                bytes.len()
            );
        }
        Err(e) => println!("FAIL: {entry}: {e}"),
    }
    Ok(())
}
