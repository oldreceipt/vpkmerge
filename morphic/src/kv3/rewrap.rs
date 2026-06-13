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

    // A binary-blob section cannot be re-emitted uncompressed in an engine-loadable
    // way. Decompressing the blob frames and flipping compressionMethod to 0 leaves
    // the now-stale per-frame size table in the buffer tail; morphic's own reader
    // ignores it when comp=0, but Source 2 still consults it and misreads the blob,
    // so the owning material loads broken and the mesh it covers renders as
    // wireframe (observed in-game on `inferno_body.vmat_c`). So a blobbed block is
    // refused here for every version, exactly as the recolor callers already expect:
    // they skip that entry and leave it vanilla rather than ship a broken file.
    // (`countBlocks` is at offset 56 for both v4 and v5.)
    if i32_at(block, 56) != 0 {
        return Err(DecodeError::Kv3(
            "re-wrap does not support a binary-blob section (not engine-loadable uncompressed)",
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
/// (main) sizes at 80/84. A binary-blob section is rejected upstream in
/// [`rewrap_uncompressed`] (it cannot be re-emitted uncompressed for the engine),
/// so only the two typed buffers are decompressed here.
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
    write_u32(&mut out, 68, 0); // sizeBlockCompressed = 0 (no frame table when raw)
    write_i32(&mut out, 76, i32_at(block, 72)); // comp_buf1 = unc_buf1
    write_i32(&mut out, 84, i32_at(block, 80)); // comp_buf2 = unc_buf2
    out.extend_from_slice(&buf1);
    out.extend_from_slice(&buf2);
    Ok(out)
}

/// True for a v5 block that is LZ4-compressed (`compressionMethod == 1`) and
/// carries a binary-blob section (`countBlocks != 0`). This is the one shape that
/// cannot be re-emitted `compressionMethod = 0` in an engine-loadable way (see the
/// refusal in [`rewrap_uncompressed`]); the in-place double patch handles it
/// instead via [`decompress_v5_working`] + [`reassemble_blobbed_v5`], keeping the
/// block compressed and the blob frames byte-identical. ZSTD-compressed blobbed
/// blocks are excluded (we have no ZSTD encoder) and still take the refusal path.
pub(crate) fn is_blobbed_lz4_v5(block: &[u8]) -> bool {
    block.len() >= 120
        && (u32_at(block, 0) & 0xFF) == 5
        && u32_at(block, 20) == 1 // LZ4
        && i32_at(block, 56) != 0 // has a binary-blob section
}

/// Decompress a v5 block's two typed buffers into a flat, **walkable**
/// uncompressed copy: `[original 120-byte header][raw buf1][raw buf2]`. The blob
/// frames are deliberately omitted: a `BINARY_BLOB` node consumes no typed-lane
/// bytes, and the in-place walkers never read past buf2's type stream, so they
/// need only the two decompressed typed buffers. The header is copied verbatim
/// (its `unc1` at offset 72 still locates buf2 at `120 + unc1`, exactly as
/// [`super::patch::lanes_v5`] expects); the stale compression fields are unread by
/// the walk. Pair with [`reassemble_blobbed_v5`] to re-emit after patching.
pub(crate) fn decompress_v5_working(block: &[u8]) -> Result<Vec<u8>, DecodeError> {
    const HEADER: usize = 120;
    if block.len() < HEADER {
        return Err(DecodeError::Truncated {
            offset: 0,
            needed: HEADER,
            had: block.len(),
        });
    }
    let compression = u32_at(block, 20);
    let unc1 = usize_at(block, 72)?;
    let comp1 = usize_at(block, 76)?;
    let unc2 = usize_at(block, 80)?;
    let comp2 = usize_at(block, 84)?;
    let b2c = HEADER
        .checked_add(comp1)
        .ok_or(DecodeError::Kv3("buffer1 extent overflow"))?;
    let raw1 = decompress(slice(block, HEADER, comp1)?, compression, unc1, comp1)?;
    let raw2 = decompress(slice(block, b2c, comp2)?, compression, unc2, comp2)?;

    let mut out = Vec::with_capacity(HEADER + raw1.len() + raw2.len());
    out.extend_from_slice(&block[..HEADER]);
    out.extend_from_slice(&raw1);
    out.extend_from_slice(&raw2);
    Ok(out)
}

/// Re-emit a compressed v5 blobbed block from a patched uncompressed working copy
/// (the `[header][raw buf1][raw buf2]` produced by [`decompress_v5_working`] and
/// then patched in place), keeping `compressionMethod = 1`.
///
/// Only the typed buffer whose raw bytes actually changed is recompressed (with
/// `lz4_flex`); the other typed buffer and the entire binary-blob frame region are
/// spliced through byte-for-byte. The blob frames are located by sequential,
/// size-derived reads (no absolute offset is stored anywhere), so rewriting the
/// buffer compressed-size fields relocates them correctly. The per-blob length
/// table, the document trailer, and the LZ4 per-frame size table all live in
/// buf2's tail and in the frame region, none of which a tint-double edit touches,
/// so they stay valid.
///
/// This is the engine-loadable alternative to flipping a blobbed block to
/// `compressionMethod = 0`: that leaves a stale per-frame size table the engine
/// still consults, so it misreads the blob and the owning material renders broken.
pub(crate) fn reassemble_blobbed_v5(orig: &[u8], working: &[u8]) -> Result<Vec<u8>, DecodeError> {
    const HEADER: usize = 120;
    let unc1 = usize_at(orig, 72)?;
    let comp1 = usize_at(orig, 76)?;
    let unc2 = usize_at(orig, 80)?;
    let comp2 = usize_at(orig, 84)?;

    let b2c = HEADER
        .checked_add(comp1)
        .ok_or(DecodeError::Kv3("buffer1 extent overflow"))?;
    let frames_start = b2c
        .checked_add(comp2)
        .ok_or(DecodeError::Kv3("buffer2 extent overflow"))?;
    let frames = slice(orig, frames_start, orig.len().saturating_sub(frames_start))?;

    // Patched raw buffers, carved from the working copy by uncompressed size.
    let raw1_start = HEADER;
    let raw2_start = HEADER
        .checked_add(unc1)
        .ok_or(DecodeError::Kv3("buffer1 raw extent overflow"))?;
    let new_raw1 = slice(working, raw1_start, unc1)?;
    let new_raw2 = slice(working, raw2_start, unc2)?;

    // Originals, so a buffer that did not change is re-emitted byte-identical.
    let orig_comp1 = slice(orig, HEADER, comp1)?;
    let orig_comp2 = slice(orig, b2c, comp2)?;
    let orig_raw1 = decompress(orig_comp1, 1, unc1, comp1)?;
    let orig_raw2 = decompress(orig_comp2, 1, unc2, comp2)?;

    let (bytes1, ncomp1) = recompress_if_changed(new_raw1, &orig_raw1, orig_comp1);
    let (bytes2, ncomp2) = recompress_if_changed(new_raw2, &orig_raw2, orig_comp2);
    let total_comp = ncomp1
        .checked_add(ncomp2)
        .ok_or(DecodeError::Kv3("compressed size overflow"))?;

    let mut out = orig[..HEADER].to_vec();
    // size_comp_total (52) is comp1 + comp2 in these files (blob frames excluded);
    // size_unc_total (48), countBlocks (56), sizeBlobs (60), sizeBlockCompressed
    // (68), blob frame size (26), and compressionMethod (20) are all unchanged: the
    // uncompressed sizes and the entire blob framing are untouched by the patch.
    write_i32(&mut out, 52, fit_i32(total_comp)?);
    write_i32(&mut out, 76, fit_i32(ncomp1)?);
    write_i32(&mut out, 84, fit_i32(ncomp2)?);
    out.extend_from_slice(&bytes1);
    out.extend_from_slice(&bytes2);
    out.extend_from_slice(frames);
    Ok(out)
}

/// Replace the sole binary blob of a blobbed-LZ4 v5 block with `new` (same
/// uncompressed length as the existing blob), re-emitting an engine-loadable
/// compressed block. Handles only the single-blob, single-LZ4-frame shape, which
/// is every Deadlock `.vnmclip_c` pose stream (one blob, < 16 KB, one frame);
/// any other shape errors so the caller can fall back rather than corrupt.
///
/// The per-frame compressed-size table is the tail of buf2 (after the type
/// stream, the per-blob length table, and the trailer), so swapping the blob
/// means: recompress it into one LZ4 frame, rewrite that one `u16` table entry,
/// recompress buf2, and fix the two affected header sizes. `old` is verified
/// against the decompressed existing frame first, so any layout surprise errors
/// out instead of shipping a broken file. The block stays `compressionMethod = 1`
/// (the engine misreads a blobbed block flipped to 0; see [`rewrap_uncompressed`]).
pub(crate) fn replace_single_blob_v5(
    orig: &[u8],
    old: &[u8],
    new: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    const HEADER: usize = 120;
    if !is_blobbed_lz4_v5(orig) {
        return Err(DecodeError::Kv3("blob replace: not a blobbed LZ4 v5 block"));
    }
    if old.len() != new.len() {
        return Err(DecodeError::Kv3("blob replace requires equal length"));
    }
    if i32_at(orig, 56) != 1 {
        return Err(DecodeError::Kv3(
            "blob replace: only a single blob is handled",
        ));
    }
    let size_blobs = usize_at(orig, 60)?;
    let size_block_compressed = usize_at(orig, 68)?;
    let comp1 = usize_at(orig, 76)?;
    let unc2 = usize_at(orig, 80)?;
    let comp2 = usize_at(orig, 84)?;
    if size_blobs != new.len() {
        return Err(DecodeError::Kv3("blob replace: new length != blob length"));
    }
    let frame_size = usize::from(u16::from_le_bytes([orig[26], orig[27]]));
    if frame_size == 0 || size_blobs > frame_size {
        return Err(DecodeError::Kv3(
            "blob replace: multi-frame blob not handled",
        ));
    }
    if size_block_compressed != 2 {
        return Err(DecodeError::Kv3(
            "blob replace: expected a one-entry frame table",
        ));
    }

    let b2c = HEADER
        .checked_add(comp1)
        .ok_or(DecodeError::Kv3("buffer1 extent overflow"))?;
    let frames_start = b2c
        .checked_add(comp2)
        .ok_or(DecodeError::Kv3("buffer2 extent overflow"))?;
    let frames = slice(orig, frames_start, orig.len().saturating_sub(frames_start))?;

    // The single frame's compressed size is the lone u16 at buf2's tail.
    let raw2 = decompress(slice(orig, b2c, comp2)?, 1, unc2, comp2)?;
    if raw2.len() < size_block_compressed {
        return Err(DecodeError::Kv3(
            "blob replace: buf2 shorter than frame table",
        ));
    }
    let table_off = raw2.len() - size_block_compressed;
    let old_frame_len = usize::from(u16::from_le_bytes([raw2[table_off], raw2[table_off + 1]]));

    // Verify the existing frame decompresses to exactly `old` (empty dictionary:
    // it is the first and only frame), so a layout mismatch fails safely.
    let frame_in = slice(frames, 0, old_frame_len)?;
    let mut decoded = vec![0u8; size_blobs];
    let n = lz4_flex::block::decompress_into(frame_in, &mut decoded)
        .map_err(|e| DecodeError::Kv3Lz4(e.to_string()))?;
    if n != size_blobs || decoded != old {
        return Err(DecodeError::Kv3(
            "blob replace: existing blob did not match `old`",
        ));
    }
    let trailing = frames.get(old_frame_len..).unwrap_or(&[]);

    // Recompress the new blob into one frame and patch the frame-table entry.
    let new_frame = lz4_flex::block::compress(new);
    let new_frame_len = u16::try_from(new_frame.len())
        .map_err(|_| DecodeError::Kv3("blob replace: frame exceeds 64 KB"))?;
    let mut raw2_new = raw2;
    raw2_new[table_off..table_off + 2].copy_from_slice(&new_frame_len.to_le_bytes());
    let comp2_new = lz4_flex::block::compress(&raw2_new);

    // comp1 (76) and unc2 (80) are unchanged; only comp2 (84) and the total
    // compressed size (52, blob frames excluded) move. Blob framing fields
    // (26/56/60/68) are untouched: one blob, one frame, same uncompressed length.
    let total_comp = comp1
        .checked_add(comp2_new.len())
        .ok_or(DecodeError::Kv3("compressed size overflow"))?;
    let mut out = orig[..HEADER].to_vec();
    write_i32(&mut out, 52, fit_i32(total_comp)?);
    write_i32(&mut out, 84, fit_i32(comp2_new.len())?);
    out.extend_from_slice(slice(orig, HEADER, comp1)?);
    out.extend_from_slice(&comp2_new);
    out.extend_from_slice(&new_frame);
    out.extend_from_slice(trailing);
    Ok(out)
}

/// Keep a buffer byte-identical when its raw bytes did not change (so an unchanged
/// buffer round-trips exactly), else LZ4-recompress the patched raw bytes.
fn recompress_if_changed(new_raw: &[u8], orig_raw: &[u8], orig_comp: &[u8]) -> (Vec<u8>, usize) {
    if new_raw == orig_raw {
        (orig_comp.to_vec(), orig_comp.len())
    } else {
        let c = lz4_flex::block::compress(new_raw);
        let n = c.len();
        (c, n)
    }
}

fn fit_i32(v: usize) -> Result<i32, DecodeError> {
    i32::try_from(v).map_err(|_| DecodeError::Kv3("size field exceeds i32"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A KV3 `DATA` block carrying a binary-blob section (`countBlocks > 0`) cannot
    /// be re-emitted uncompressed in an engine-loadable form, so the re-wrap must
    /// refuse it rather than produce a file that only morphic's lenient reader
    /// accepts (a blobbed `inferno_body.vmat_c` patched this way rendered the hero's
    /// upper body as wireframe in-game). The refusal is what lets the recolor caller
    /// skip the entry and leave it vanilla. Regression guard for that path, which had
    /// no coverage when the v5 blob pass-through was (wrongly) added.
    #[test]
    fn rewrap_refuses_a_binary_blob_section() {
        // Minimal v5 header: magic (v5) + a nonzero compressionMethod (so it is not
        // a no-op pass-through) + a nonzero countBlocks at offset 56. The blob check
        // fires before any buffer math, so the rest of the header can stay zero.
        let mut block = vec![0u8; 120];
        write_u32(&mut block, 0, MAGIC_BASE | 5); // KV3 v5
        write_u32(&mut block, 20, 1); // compressionMethod = LZ4 (nonzero)
        write_i32(&mut block, 56, 1); // countBlocks = 1 (has a blob section)

        let err = rewrap_uncompressed(&block).expect_err("blobbed block must be refused");
        assert!(
            matches!(err, DecodeError::Kv3(msg) if msg.contains("binary-blob section")),
            "expected a binary-blob-section refusal, got {err:?}"
        );
    }

    /// The same header with `countBlocks == 0` gets past the blob guard (it then
    /// proceeds to buffer decompression), proving the guard keys on the blob count,
    /// not on something incidental to the synthetic header.
    #[test]
    fn rewrap_blob_guard_keys_on_count_blocks() {
        let mut block = vec![0u8; 120];
        write_u32(&mut block, 0, MAGIC_BASE | 5);
        write_u32(&mut block, 20, 1);
        write_i32(&mut block, 56, 0); // no blob section

        let err = rewrap_uncompressed(&block).expect_err("zero-size LZ4 buffers still error");
        assert!(
            !matches!(err, DecodeError::Kv3(msg) if msg.contains("binary-blob section")),
            "a non-blobbed block must not trip the blob guard, got {err:?}"
        );
    }
}
