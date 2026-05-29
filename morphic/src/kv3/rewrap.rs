//! Faithful KV3 re-wrap: take a compiled binary KV3 block and re-emit it
//! **uncompressed**, preserving every other byte of structure verbatim (type
//! stream, value flags, typed-array tags, string table, scalar lanes).
//!
//! This exists because the generic [`super::writer`] re-encodes from the decoded
//! [`Value`](super::Value) tree, which is lossy in two ways the engine's *model*
//! loader does not tolerate (even though the soundevents loader did): value flags
//! (e.g. the `resource` flag on `m_material`) are dropped, and auxiliary-buffer /
//! typed arrays are flattened to generic arrays. A model `MDAT` block uses both
//! heavily. Re-wrapping the original buffers without going through `Value` keeps
//! them bit-for-bit; only the compression method changes (the engine reads
//! `compressionMethod = 0` buffers directly, as proven for soundevents v4).
//!
//! Only the no-blob case is handled (model `MDAT`/`DATA`/`CTRL` carry
//! `countBlocks == 0`; `ANIM` blobs are out of scope and rejected). Already
//! uncompressed input is returned unchanged.

use super::Format;
use crate::error::DecodeError;

const MAGIC_BASE: u32 = 0x4B56_3300;

/// Re-emit `block` (a compiled binary KV3 payload, v1..=5) with its buffers
/// decompressed in place: same version, same header, same buffer bytes, but
/// `compressionMethod = 0`. The decoded value tree is identical to the original's.
///
/// Errors on an unsupported version, a blob section (`countBlocks > 0`), or an
/// unknown compression method.
pub fn rewrap_uncompressed(block: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if block.len() < 24 {
        return Err(DecodeError::Truncated {
            offset: 0,
            needed: 24,
            had: block.len(),
        });
    }
    let magic = u32_at(block, 0);
    let version = magic & 0xFF;
    if magic & 0xFFFF_FF00 != MAGIC_BASE || !(1..=5).contains(&version) {
        return Err(DecodeError::UnsupportedKv3(magic));
    }
    let compression = u32_at(block, 20);

    // v1 has no compression field path here (it is always uncompressed and uses a
    // shorter header); we only ever re-wrap v4/v5 model blocks. Treat already
    // uncompressed input as a no-op for every version.
    if compression == 0 {
        return Ok(block.to_vec());
    }
    if version < 4 {
        return Err(DecodeError::Kv3("re-wrap supports only KV3 v4/v5"));
    }

    let count_blocks = i32_at(block, 56);
    if count_blocks != 0 {
        return Err(DecodeError::Kv3(
            "re-wrap does not support KV3 blocks with a binary-blob section",
        ));
    }

    if version == 5 {
        rewrap_v5(block, compression)
    } else {
        rewrap_v4(block, compression)
    }
}

/// v4 single-buffer: `[header(72)][buffer]`. `size_unc_total`/`size_comp_total`
/// at offsets 48/52 describe the one buffer.
fn rewrap_v4(block: &[u8], compression: u32) -> Result<Vec<u8>, DecodeError> {
    const HEADER: usize = 72;
    let size_unc = usize_at(block, 48)?;
    let size_comp = usize_at(block, 52)?;
    let buf = decompress(
        block.get(HEADER..).unwrap_or(&[]),
        compression,
        size_unc,
        size_comp,
    )?;

    let mut out = block[..HEADER].to_vec();
    write_u32(&mut out, 20, 0); // compressionMethod = 0
    write_u16(&mut out, 26, 0); // frame size (u16) unused when uncompressed
    write_i32(&mut out, 52, i32_at(block, 48)); // size_comp_total = size_unc_total
    write_u32(&mut out, 68, 0); // sizeBlockCompressed = 0
    out.extend_from_slice(&buf);
    Ok(out)
}

/// v5 two-buffer: `[header(120)][buf1][buf2]`. buf1 (aux) sizes at 72/76, buf2
/// (main) sizes at 80/84.
fn rewrap_v5(block: &[u8], compression: u32) -> Result<Vec<u8>, DecodeError> {
    const HEADER: usize = 120;
    let unc1 = usize_at(block, 72)?;
    let comp1 = usize_at(block, 76)?;
    let unc2 = usize_at(block, 80)?;
    let comp2 = usize_at(block, 84)?;

    let b1_start = HEADER;
    let b2_start = b1_start
        .checked_add(comp1)
        .ok_or(DecodeError::Kv3("buffer1 extent overflow"))?;
    let buf1 = decompress(slice(block, b1_start, comp1)?, compression, unc1, comp1)?;
    let buf2 = decompress(slice(block, b2_start, comp2)?, compression, unc2, comp2)?;

    let mut out = block[..HEADER].to_vec();
    write_u32(&mut out, 20, 0); // compressionMethod = 0
    write_u16(&mut out, 26, 0); // frame size (u16) unused
    write_i32(&mut out, 52, i32_at(block, 48)); // size_comp_total = size_unc_total
    write_u32(&mut out, 68, 0); // sizeBlockCompressed = 0
    write_i32(&mut out, 76, i32_at(block, 72)); // comp_buf1 = unc_buf1
    write_i32(&mut out, 84, i32_at(block, 80)); // comp_buf2 = unc_buf2
    out.extend_from_slice(&buf1);
    out.extend_from_slice(&buf2);
    Ok(out)
}

fn decompress(
    input: &[u8],
    compression: u32,
    size_unc: usize,
    size_comp: usize,
) -> Result<Vec<u8>, DecodeError> {
    match compression {
        0 => Ok(input.get(..size_unc).unwrap_or(input).to_vec()),
        1 => {
            let src = input
                .get(..size_comp)
                .ok_or(DecodeError::Kv3("LZ4 input underrun"))?;
            let mut out = vec![0u8; size_unc];
            let n = lz4_flex::block::decompress_into(src, &mut out)
                .map_err(|e| DecodeError::Kv3Lz4(e.to_string()))?;
            if n != size_unc {
                return Err(DecodeError::Kv3Lz4(format!(
                    "expected {size_unc} bytes, got {n}"
                )));
            }
            Ok(out)
        }
        2 => {
            use std::io::Read;
            let src = input
                .get(..size_comp)
                .ok_or(DecodeError::Kv3("ZSTD input underrun"))?;
            let mut dec = ruzstd::decoding::StreamingDecoder::new(src)
                .map_err(|_| DecodeError::Kv3("ZSTD init failed"))?;
            let mut out = vec![0u8; size_unc];
            dec.read_exact(&mut out)
                .map_err(|_| DecodeError::Kv3("ZSTD decompress failed"))?;
            Ok(out)
        }
        other => Err(DecodeError::Kv3Compression(other)),
    }
}

/// Reads the 16-byte format GUID a re-wrapped block carries (unchanged from the
/// original). Lets callers keep the block's schema id when splicing.
#[allow(dead_code)]
pub fn format_of(block: &[u8]) -> Result<Format, DecodeError> {
    Format::from_payload(block)
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn usize_at(b: &[u8], o: usize) -> Result<usize, DecodeError> {
    usize::try_from(i32_at(b, o)).map_err(|_| DecodeError::Kv3("negative size field"))
}

fn write_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn write_u16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn write_i32(b: &mut [u8], o: usize, v: i32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn slice(b: &[u8], start: usize, len: usize) -> Result<&[u8], DecodeError> {
    let end = start.checked_add(len).ok_or(DecodeError::Kv3("overflow"))?;
    b.get(start..end)
        .ok_or(DecodeError::Kv3("buffer slice out of range"))
}
