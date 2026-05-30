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

    // A v4 binary-blob section is unsupported (the reader rejects v4 blobs too);
    // v5 blobs are decompressed and passed through by `rewrap_v5`.
    if version == 4 && i32_at(block, 56) != 0 {
        return Err(DecodeError::Kv3(
            "re-wrap does not support a binary-blob section in a KV3 v4 block",
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
    let count_blocks = i32_at(block, 56);

    let b1_start = HEADER;
    let b2_start = b1_start
        .checked_add(comp1)
        .ok_or(DecodeError::Kv3("buffer1 extent overflow"))?;
    let buf1 = decompress(slice(block, b1_start, comp1)?, compression, unc1, comp1)?;
    let buf2 = decompress(slice(block, b2_start, comp2)?, compression, unc2, comp2)?;

    // Binary-blob section (some compiled materials/models carry e.g. precompiled
    // data). The compressed blob frames sit in the stream right after buf2; their
    // per-frame size table lives in buf2's tail. Decompress them to raw bytes so the
    // comp=0 re-emit can append them verbatim (the reader reads raw blobs when
    // comp=0, ignoring the now-unused frame table left in buf2's tail).
    let blobs = if count_blocks > 0 {
        let frames_start = b2_start
            .checked_add(comp2)
            .ok_or(DecodeError::Kv3("buffer2 extent overflow"))?;
        decompress_v5_blobs(block, &buf2, compression, frames_start)?
    } else {
        Vec::new()
    };

    let mut out = block[..HEADER].to_vec();
    write_u32(&mut out, 20, 0); // compressionMethod = 0
    write_u16(&mut out, 26, 0); // frame size (u16) unused
    write_i32(&mut out, 52, i32_at(block, 48)); // size_comp_total = size_unc_total
    write_u32(&mut out, 68, 0); // sizeBlockCompressed = 0 (no frame table when raw)
    write_i32(&mut out, 76, i32_at(block, 72)); // comp_buf1 = unc_buf1
    write_i32(&mut out, 84, i32_at(block, 80)); // comp_buf2 = unc_buf2
    out.extend_from_slice(&buf1);
    out.extend_from_slice(&buf2);
    out.extend_from_slice(&blobs);
    Ok(out)
}

/// Decompress a v5 block's binary-blob section to its raw concatenated bytes
/// (`size_blobs` total). Mirrors `reader::read_blobs`: the per-blob length table +
/// trailer + (for LZ4) the per-frame compressed-size table sit in `buf2`'s tail
/// just past the type stream; the compressed frames themselves start at
/// `frames_start` in `block`.
fn decompress_v5_blobs(
    block: &[u8],
    buf2: &[u8],
    compression: u32,
    frames_start: usize,
) -> Result<Vec<u8>, DecodeError> {
    let size_blobs = usize_at(block, 60)?;
    let count_blocks = usize_at(block, 56)?;
    let after_types = v5_after_types(block)?;
    // [blob lengths: count_blocks * i32][trailer: 4][frame table].
    let table_off = after_types
        .checked_add(count_blocks.checked_mul(4).ok_or(over())?)
        .and_then(|x| x.checked_add(4))
        .ok_or(over())?;

    match compression {
        1 => {
            let sbc = usize_at(block, 68)?;
            let table = slice(buf2, table_off, sbc)?;
            let frame_size = u16::from_le_bytes([block[26], block[27]]) as usize;
            if frame_size == 0 {
                return Err(DecodeError::Kv3("zero blob frame size"));
            }
            let mut out = vec![0u8; size_blobs];
            let mut done = 0usize;
            let mut sp = frames_start;
            for fs in table.chunks_exact(2) {
                if done >= size_blobs {
                    break;
                }
                let comp = usize::from(u16::from_le_bytes([fs[0], fs[1]]));
                let want = frame_size.min(size_blobs - done);
                let input = slice(block, sp, comp)?;
                sp = sp.checked_add(comp).ok_or(over())?;
                let (dict, rest) = out.split_at_mut(done);
                let n = lz4_flex::block::decompress_into_with_dict(input, &mut rest[..want], dict)
                    .map_err(|e| DecodeError::Kv3Lz4(e.to_string()))?;
                done += n;
            }
            if done != size_blobs {
                return Err(DecodeError::Kv3("blob size mismatch"));
            }
            Ok(out)
        }
        // v5 ZSTD: one frame for the whole region (no per-frame table).
        2 => decompress(
            slice(block, frames_start, block.len() - frames_start)?,
            2,
            size_blobs,
            block.len() - frames_start,
        ),
        other => Err(DecodeError::Kv3Compression(other)),
    }
}

/// Byte offset in `buf2` just past the v5 type stream (where the blob length table
/// begins). Mirrors `reader::layout_main_v5`'s offset math.
fn v5_after_types(block: &[u8]) -> Result<usize, DecodeError> {
    let obj = usize_at(block, 108)?;
    let b1 = usize_at(block, 88)?;
    let b2 = usize_at(block, 92)?;
    let b4 = usize_at(block, 96)?;
    let b8 = usize_at(block, 100)?;
    let count_types = usize_at(block, 40)?;
    let mut off = obj
        .checked_mul(4)
        .ok_or(over())?
        .checked_add(b1)
        .ok_or(over())?;
    off = adv(off, b2, 2);
    off = adv(off, b4, 4);
    off = adv(off, b8, 8);
    off.checked_add(count_types).ok_or(over())
}

/// Advance past an aligned typed lane of `count` items of `elem` bytes (no
/// alignment when the lane is empty, matching `reader::lane`).
fn adv(off: usize, count: usize, elem: usize) -> usize {
    if count == 0 {
        return off;
    }
    ((off + elem - 1) & !(elem - 1)) + count * elem
}

fn over() -> DecodeError {
    DecodeError::Kv3("blob layout offset overflow")
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
