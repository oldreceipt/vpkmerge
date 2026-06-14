//! Emit loose `.vnmclip_c` files to open in Source2Viewer (engine-grade VRF KV3
//! reader) to bisect the "Bad KV3 data" the engine reported for re-encoded clips:
//!  - original.vnmclip_c   : untouched control (must load fine)
//!  - resize_only.vnmclip_c: blob extended via patch_kv3_resource_sole_blob ONLY
//!                           (no offset/flag patches) -> isolates the blob resize
//!  - finger.vnmclip_c     : the full channel-add (resize + set_scalars + set_bools)
//! If resize_only loads but finger errors, the offset/flag patches are the culprit;
//! if resize_only also errors, the blob-resize container write is.
//!
//! Usage: cargo run --release -p vpkmerge-core --example dump_test_clips -- <pak01_dir.vpk> <out_dir> [finger_addon_dir.vpk]

use anyhow::{Context, Result};
use morphic::model::decode_nm_clip;

const CLIP: &str = "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c";

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().context("pak")?;
    let out = a.next().context("out_dir")?;
    let addon = a.next();

    let bytes = vpkmerge_core::read_vpk_entry(&pak, CLIP)?;
    std::fs::write(format!("{out}/original.vnmclip_c"), &bytes)?;
    println!("wrote original.vnmclip_c ({} bytes)", bytes.len());

    // resize-only: append bytes to the blob, nothing else.
    let clip = decode_nm_clip(&bytes)?;
    let mut bigger = clip.compressed_pose_data.clone();
    bigger.extend(std::iter::repeat_n(0xABu8, 512));
    let resize = morphic::patch_kv3_resource_sole_blob(&bytes, &bigger)?;
    std::fs::write(format!("{out}/resize_only.vnmclip_c"), &resize)?;
    println!(
        "wrote resize_only.vnmclip_c ({} bytes, blob +512)",
        resize.len()
    );

    // Pack each into a standalone VPK too (Source2Viewer opens a VPK cleanly; a
    // loose file crashes its game-root resolver).
    vpkmerge_core::pack(
        &[(CLIP, resize.as_slice())],
        &format!("{out}/resize_only_dir.vpk"),
    )?;
    vpkmerge_core::pack(
        &[(CLIP, bytes.as_slice())],
        &format!("{out}/original_dir.vpk"),
    )?;
    println!("packed original_dir.vpk + resize_only_dir.vpk");

    // the full channel-add clip, pulled from the finger-test addon if given.
    if let Some(addon) = addon {
        let finger = vpkmerge_core::read_vpk_entry(&addon, CLIP)?;
        std::fs::write(format!("{out}/finger.vnmclip_c"), &finger)?;
        println!("wrote finger.vnmclip_c ({} bytes)", finger.len());
    }

    println!("\nopen each in Source2Viewer (File > Open) and report which load vs error.");
    Ok(())
}
