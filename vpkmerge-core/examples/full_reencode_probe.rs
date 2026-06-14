//! Gate test for the full-re-encode strategy: decode a `.vnmclip_c`'s DATA block
//! to a KV3 value tree and re-encode it via `encode_kv3_resource` (uncompressed
//! v4) with NO edits. If the engine/VRF accepts this identity re-encode, then a
//! full re-encode is viable for the encoder gaps (translation/scale channel adds,
//! frame-count changes) which are structural and can't be done by in-place
//! patching. If it's rejected, those gaps need another approach. The risk is that
//! the generic writer drops value flags (e.g. the resource flag on `m_skeleton`)
//! or typed-array tags the engine requires.
//!
//! Packs the result into a VPK under the Deadlock addons/inspect/ tree so it can
//! be opened in Source2Viewer (engine-grade KV3 reader) without the loose-file crash.
//!
//! Usage: cargo run --release -p vpkmerge-core --example full_reencode_probe -- <pak01_dir.vpk> <out_dir>

use anyhow::{Context, Result};

const CLIP: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().context("pak")?;
    let out = a.next().context("out_dir")?;

    let bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    let tree = morphic::decode_kv3_resource(&bytes).context("decode DATA tree")?;
    let reenc = morphic::encode_kv3_resource(&bytes, &tree).context("re-encode v4")?;
    println!(
        "identity full re-encode: {} -> {} bytes (v4 uncompressed)",
        bytes.len(),
        reenc.len()
    );

    let vpk = format!("{out}/full_reencode_dir.vpk");
    vpkmerge_core::pack(&[(CLIP, reenc.as_slice())], &vpk)?;
    println!("packed -> {vpk}");
    println!("open in Source2Viewer: does reload_idle_quick.vnmclip_c decode, or error?");
    Ok(())
}
