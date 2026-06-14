//! Probe how a `.vnmclip_c`'s `m_compressedPoseData` blob is stored in the v5
//! container: dump the DATA block header (compression method, buffer sizes, blob
//! section count) and check whether the raw decoded blob bytes appear verbatim in
//! the resource (i.e. the blob is stored uncompressed and can be spliced directly)
//! or are LZ4-framed (needs a blob-frame re-encode). Throwaway dev tool.
//!
//! Usage: cargo run --release -p vpkmerge-core --example nm_blob_probe -- <pak01_dir.vpk> <entry>

use anyhow::{Context, Result};
use morphic::model::decode_nm_clip;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|&i| &hay[i..i + needle.len()] == needle)
        .collect()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing pak")?;
    let entry = args.next().context("missing entry")?;
    let bytes = vpkmerge_core::read_vpk_entry(&pak, &entry)?;
    let clip = decode_nm_clip(&bytes)?;
    println!(
        "{entry}\n  resource {} bytes | blob {} bytes | {} frames",
        bytes.len(),
        clip.compressed_pose_data.len(),
        clip.frame_count
    );

    // The DATA block starts somewhere in the resource; we don't have its offset
    // here, so just scan the whole resource for the v5 header magic-ish fields by
    // checking where the blob bytes land instead.
    let hits = find_all(&bytes, &clip.compressed_pose_data);
    println!(
        "  raw blob occurs {} time(s) in the resource {}",
        hits.len(),
        if hits.len() == 1 {
            "(splice-able raw!)"
        } else if hits.is_empty() {
            "(LZ4-framed; needs frame re-encode)"
        } else {
            "(ambiguous)"
        }
    );
    for h in &hits {
        println!("    at byte {h}");
    }

    // Also dump the v5 DATA header if we can find it: search for a plausible v5
    // header by scanning for the version byte pattern is unreliable, so just print
    // the first 96 bytes after the typical resource header for a human to read.
    if bytes.len() > 200 {
        let tail = &bytes[bytes
            .len()
            .saturating_sub(clip.compressed_pose_data.len() + 64)..];
        println!("  64 bytes preceding the blob tail region:");
        print!("    ");
        for b in tail.iter().take(64) {
            print!("{b:02x} ");
        }
        println!();
    }
    let _ = u32_at; // (kept for ad-hoc header poking)
    Ok(())
}
