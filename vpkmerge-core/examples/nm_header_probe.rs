//! Dump a `.vnmclip_c` DATA block's v5 KV3 header size fields, to check which
//! totals depend on sizeBlobs (so replace_blob_v5 updates them all). Finds the v5
//! magic in the resource and prints the size fields + the size_unc_total relation.
//! Usage: cargo run --release -p vpkmerge-core --example nm_header_probe -- <pak> <entry>

use anyhow::{Context, Result};

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn main() -> Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().context("pak")?;
    let entry = a.next().context("entry")?;
    let bytes = vpkmerge_core::read_vpk_entry(&pak, &entry)?;

    // v5 KV3 magic: 0x4B563305 -> LE bytes 05 33 56 4B. Scan all blocks; the pose
    // DATA block is the one with countBlocks != 0 (it carries the blob).
    let magic = [0x05u8, 0x33, 0x56, 0x4B];
    let all: Vec<usize> = (0..bytes.len().saturating_sub(120))
        .filter(|&i| bytes[i..i + 4] == magic)
        .collect();
    println!("v5 KV3 blocks at bytes {all:?}");
    let h = all
        .iter()
        .copied()
        .find(|&i| u32_at(&bytes, i + 56) != 0)
        .context("no blobbed v5 block found")?;
    println!("blobbed DATA block at byte {h}");
    let f = |o: usize| u32_at(&bytes, h + o);
    let unc_total = f(48);
    let comp_total = f(52);
    let count_blocks = f(56);
    let size_blobs = f(60);
    let size_block_compressed = f(68);
    let unc1 = f(72);
    let comp1 = f(76);
    let unc2 = f(80);
    let comp2 = f(84);
    println!("  48 size_unc_total      = {unc_total}");
    println!("  52 size_comp_total     = {comp_total}");
    println!("  56 countBlocks         = {count_blocks}");
    println!("  60 sizeBlobs           = {size_blobs}");
    println!("  68 size_block_compressed = {size_block_compressed}");
    println!("  72 unc1 = {unc1}  76 comp1 = {comp1}");
    println!("  80 unc2 = {unc2}  84 comp2 = {comp2}");
    println!("  --- relations ---");
    println!("  unc1+unc2           = {}", unc1 + unc2);
    println!("  unc1+unc2+sizeBlobs = {}", unc1 + unc2 + size_blobs);
    println!("  comp1+comp2         = {}", comp1 + comp2);
    println!(
        "  => size_unc_total matches {}",
        if unc_total == unc1 + unc2 + size_blobs {
            "unc1+unc2+sizeBlobs"
        } else if unc_total == unc1 + unc2 {
            "unc1+unc2 (blobs excluded)"
        } else {
            "NEITHER (?)"
        }
    );
    Ok(())
}
