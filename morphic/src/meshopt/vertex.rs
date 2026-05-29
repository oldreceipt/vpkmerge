//! meshoptimizer vertex buffer decoder (codec v1), a port of
//! `ValveResourceFormat.Compression.MeshOptimizerVertexDecoder` (scalar path),
//! itself a port of zeux's `meshoptimizer/src/vertexcodec.cpp`. Source 2 `MVTX`
//! blocks store the interleaved vertex stream in this format (first byte `0xa1`
//! = vertex codec v1). Output is the raw `vertex_count * vertex_size` byte
//! stream; deinterleaving per the input layout happens later (mesh assembly).

// Faithful port of zeux's meshoptimizer vertex codec (via VRF). The truncating
// casts, index-based delta loops, and bit-magic constants mirror the reference
// and are intentional, so the matching pedantic lints are allowed here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
    clippy::unusual_byte_groupings
)]

use crate::error::DecodeError;

const VERTEX_HEADER: u8 = 0xa0;
const DECODE_VERTEX_VERSION: u8 = 1;
const VERTEX_BLOCK_SIZE_BYTES: usize = 8192;
const VERTEX_BLOCK_MAX_SIZE: usize = 256;
const BYTE_GROUP_SIZE: usize = 16;
const BYTE_GROUP_DECODE_LIMIT: usize = 24;
const TAIL_MIN_SIZE_V0: usize = 32;
const TAIL_MIN_SIZE_V1: usize = 24;

const BITS_V0: [i32; 4] = [0, 2, 4, 8];
const BITS_V1: [i32; 5] = [0, 1, 2, 4, 8];

fn get_vertex_block_size(vertex_size: usize) -> usize {
    let mut result = VERTEX_BLOCK_SIZE_BYTES / vertex_size;
    result &= !(BYTE_GROUP_SIZE - 1);
    result.min(VERTEX_BLOCK_MAX_SIZE)
}

#[inline]
fn rotate32(v: u32, r: u32) -> u32 {
    (v << r) | (v >> ((32 - r) & 31))
}

#[inline]
fn unzigzag8(v: u8) -> u8 {
    (0u8.wrapping_sub(v & 1)) ^ (v >> 1)
}

#[inline]
fn unzigzag16(v: u16) -> u16 {
    (0u16.wrapping_sub(v & 1)) ^ (v >> 1)
}

/// Reads the next value from byte-group bitstream `b`, drawing an overflow byte
/// from `data[*data_var]` when all `bits` are set. Mirrors the local `Next`
/// closure in VRF's `DecodeBytesGroup`.
#[inline]
fn next_val(b: &mut u8, bits: u8, data: &[u8], data_var: &mut usize) -> u8 {
    let encv = data[*data_var];
    let mut enc = *b;
    enc >>= 8 - bits;
    *b <<= bits;
    if enc == (1u8 << bits) - 1 {
        *data_var += 1;
        return encv;
    }
    enc
}

/// Decodes one 16-value byte group at bit width `bits` (0/1/2/4/8). Returns the
/// remaining input after the group.
fn decode_bytes_group<'a>(
    data: &'a [u8],
    dest: &mut [u8],
    bits: i32,
) -> Result<&'a [u8], DecodeError> {
    match bits {
        0 => {
            for d in dest.iter_mut().take(BYTE_GROUP_SIZE) {
                *d = 0;
            }
            Ok(data)
        }
        1 => {
            let mut data_var = 2usize;
            // 8 1-bit values per byte, bit order reversed within the byte.
            let mut b = reverse_byte_bits(data[0]);
            for d in dest.iter_mut().take(8) {
                *d = next_val(&mut b, 1, data, &mut data_var);
            }
            b = reverse_byte_bits(data[1]);
            for d in dest.iter_mut().skip(8).take(8) {
                *d = next_val(&mut b, 1, data, &mut data_var);
            }
            Ok(&data[data_var..])
        }
        2 => {
            let mut data_var = 4usize;
            for g in 0..4 {
                let mut b = data[g];
                for k in 0..4 {
                    dest[g * 4 + k] = next_val(&mut b, 2, data, &mut data_var);
                }
            }
            Ok(&data[data_var..])
        }
        4 => {
            let mut data_var = 8usize;
            for g in 0..8 {
                let mut b = data[g];
                for k in 0..2 {
                    dest[g * 2 + k] = next_val(&mut b, 4, data, &mut data_var);
                }
            }
            Ok(&data[data_var..])
        }
        8 => {
            dest[..BYTE_GROUP_SIZE].copy_from_slice(&data[..BYTE_GROUP_SIZE]);
            Ok(&data[BYTE_GROUP_SIZE..])
        }
        _ => Err(DecodeError::Meshopt("unexpected meshopt bit length")),
    }
}

/// Reverses the 8 bits of a byte (the SWAR trick VRF uses for 1-bit groups).
#[inline]
fn reverse_byte_bits(b: u8) -> u8 {
    (((u64::from(b).wrapping_mul(0x8020_0802)) & 0x0884_4221_10).wrapping_mul(0x0101_0101_01) >> 32)
        as u8
}

fn decode_bytes<'a>(
    data: &'a [u8],
    dest: &mut [u8],
    bits: &[i32],
) -> Result<&'a [u8], DecodeError> {
    if !dest.len().is_multiple_of(BYTE_GROUP_SIZE) {
        return Err(DecodeError::Meshopt(
            "decode length not a multiple of group",
        ));
    }
    let header_size = (dest.len() / BYTE_GROUP_SIZE).div_ceil(4);
    let header = &data[..header_size];
    let mut data = &data[header_size..];

    let mut i = 0usize;
    while i < dest.len() {
        if data.len() < BYTE_GROUP_DECODE_LIMIT {
            return Err(DecodeError::Meshopt("meshopt input exhausted"));
        }
        let header_offset = i / BYTE_GROUP_SIZE;
        let bitsk = (header[header_offset / 4] >> ((header_offset % 4) * 2)) & 3;
        data = decode_bytes_group(
            data,
            &mut dest[i..i + BYTE_GROUP_SIZE],
            bits[bitsk as usize],
        )?;
        i += BYTE_GROUP_SIZE;
    }
    Ok(data)
}

/// Undoes the per-channel delta/xor transform for one 4-byte lane, writing the
/// reconstructed bytes interleaved into `transposed` (already offset to lane k).
fn decode_deltas1(
    size: usize,
    buffer: &[u8],
    transposed: &mut [u8],
    vertex_count: usize,
    vertex_size: usize,
    last_vertex: &[u8],
    rot: u32,
) {
    let mut buffer = buffer;
    let mut last_vertex = last_vertex;

    let mut k = 0usize;
    while k < 4 {
        let mut vertex_offset = k;

        let mut p = u32::from(last_vertex[0]);
        for j in 1..size {
            p |= u32::from(last_vertex[j]) << (8 * j);
        }

        for i in 0..vertex_count {
            let mut v = u32::from(buffer[i]);
            for j in 1..size {
                v |= u32::from(buffer[i + vertex_count * j]) << (8 * j);
            }

            v = match size {
                1 => u32::from(unzigzag8(v as u8)).wrapping_add(p),
                2 => u32::from(unzigzag16(v as u16)).wrapping_add(p),
                4 => rotate32(v, rot) ^ p,
                _ => unreachable!(),
            };

            for j in 0..size {
                transposed[vertex_offset + j] = (v >> (j * 8)) as u8;
            }

            p = v;
            vertex_offset += vertex_size;
        }

        buffer = &buffer[vertex_count * size..];
        last_vertex = &last_vertex[size..];
        k += size;
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_vertex_block<'a>(
    data: &'a [u8],
    vertex_data: &mut [u8],
    vertex_count: usize,
    vertex_size: usize,
    last_vertex: &mut [u8],
    channels: &[u8],
    version: u32,
) -> Result<&'a [u8], DecodeError> {
    if vertex_count == 0 || vertex_count > VERTEX_BLOCK_MAX_SIZE {
        return Err(DecodeError::Meshopt("invalid meshopt vertex block size"));
    }

    let mut buffer = [0u8; VERTEX_BLOCK_MAX_SIZE * 4];
    let mut transposed = [0u8; VERTEX_BLOCK_SIZE_BYTES];

    let vertex_count_aligned = (vertex_count + BYTE_GROUP_SIZE - 1) & !(BYTE_GROUP_SIZE - 1);
    let control_size = if version == 0 { 0 } else { vertex_size / 4 };

    let control = &data[..control_size];
    let mut data = &data[control_size..];

    let mut k = 0usize;
    while k < vertex_size {
        let ctrl_byte = if version == 0 { 0u8 } else { control[k / 4] };

        for j in 0..4 {
            let ctrl = (ctrl_byte >> (j * 2)) & 3;
            let region = j * vertex_count;

            if ctrl == 3 {
                // literal
                if data.len() < vertex_count {
                    return Err(DecodeError::Meshopt("meshopt literal underrun"));
                }
                buffer[region..region + vertex_count].copy_from_slice(&data[..vertex_count]);
                data = &data[vertex_count..];
            } else if ctrl == 2 {
                // zero
                buffer[region..region + vertex_count].fill(0);
            } else {
                let bits = if version == 0 {
                    &BITS_V0[..]
                } else {
                    &BITS_V1[ctrl as usize..]
                };
                data = decode_bytes(
                    data,
                    &mut buffer[region..region + vertex_count_aligned],
                    bits,
                )?;
            }
        }

        let channel = if version == 0 { 0u8 } else { channels[k / 4] };
        match channel & 3 {
            0 => decode_deltas1(
                1,
                &buffer,
                &mut transposed[k..],
                vertex_count,
                vertex_size,
                &last_vertex[k..],
                0,
            ),
            1 => decode_deltas1(
                2,
                &buffer,
                &mut transposed[k..],
                vertex_count,
                vertex_size,
                &last_vertex[k..],
                0,
            ),
            2 => decode_deltas1(
                4,
                &buffer,
                &mut transposed[k..],
                vertex_count,
                vertex_size,
                &last_vertex[k..],
                (32 - (u32::from(channel) >> 4)) & 31,
            ),
            _ => return Err(DecodeError::Meshopt("invalid meshopt channel type")),
        }

        k += 4;
    }

    vertex_data[..vertex_count * vertex_size]
        .copy_from_slice(&transposed[..vertex_count * vertex_size]);
    last_vertex
        .copy_from_slice(&transposed[vertex_size * (vertex_count - 1)..vertex_size * vertex_count]);

    Ok(data)
}

/// Decodes a meshoptimizer-compressed vertex buffer into the raw
/// `vertex_count * vertex_size` interleaved byte stream.
pub fn decode_vertex_buffer(
    vertex_count: usize,
    vertex_size: usize,
    buffer: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    if vertex_size == 0 || vertex_size > 256 {
        return Err(DecodeError::Meshopt("vertex size out of range"));
    }
    if !vertex_size.is_multiple_of(4) {
        return Err(DecodeError::Meshopt("vertex size not a multiple of 4"));
    }
    if buffer.is_empty() {
        return Err(DecodeError::Meshopt("vertex buffer too short"));
    }
    if buffer[0] & 0xF0 != VERTEX_HEADER {
        return Err(DecodeError::Meshopt("bad vertex buffer header"));
    }
    if buffer[0] & 0x0F > DECODE_VERTEX_VERSION {
        return Err(DecodeError::Meshopt("unsupported vertex codec version"));
    }
    let version = u32::from(buffer[0] & 0x0F);
    let mut data = &buffer[1..];

    let tail_size = vertex_size + if version == 0 { 0 } else { vertex_size / 4 };
    let tail_size_min = if version == 0 {
        TAIL_MIN_SIZE_V0
    } else {
        TAIL_MIN_SIZE_V1
    };
    let tail_size_padded = tail_size.max(tail_size_min);
    if data.len() < tail_size_padded {
        return Err(DecodeError::Meshopt("vertex buffer missing tail"));
    }

    let mut result = vec![0u8; vertex_count * vertex_size];

    // The tail holds the seed "last vertex" (mutated as blocks decode) and, for
    // v1, the per-lane channel descriptors.
    let tail_start = data.len() - tail_size;
    let mut last_vertex = data[tail_start..tail_start + vertex_size].to_vec();
    let channels: Vec<u8> = if version == 0 {
        Vec::new()
    } else {
        data[tail_start + vertex_size..tail_start + vertex_size + vertex_size / 4].to_vec()
    };

    let vertex_block_size = get_vertex_block_size(vertex_size);
    let mut vertex_offset = 0usize;

    while vertex_offset < vertex_count {
        let block_size = if vertex_offset + vertex_block_size < vertex_count {
            vertex_block_size
        } else {
            vertex_count - vertex_offset
        };

        let start = vertex_offset * vertex_size;
        data = decode_vertex_block(
            data,
            &mut result[start..start + block_size * vertex_size],
            block_size,
            vertex_size,
            &mut last_vertex,
            &channels,
            version,
        )?;

        vertex_offset += block_size;
    }

    if data.len() != tail_size_padded {
        return Err(DecodeError::Meshopt("vertex decode tail mismatch"));
    }
    Ok(result)
}

/// Inverse of [`unzigzag8`]: maps an unsigned byte-delta back to its zigzag
/// code. `unzigzag8(zigzag8(d)) == d` for all `d` (exhaustively tested).
#[inline]
fn zigzag8(d: u8) -> u8 {
    if d < 128 {
        d << 1
    } else {
        ((255 - d) << 1) | 1
    }
}

/// Encodes one vertex block. `vertices` is the block's `block_size * vertex_size`
/// interleaved bytes; `last_vertex` is the running seed (the previous vertex,
/// mutated to this block's last vertex on return). Uses the always-valid
/// encoding: every byte lane is a literal (control nibble `3`) carrying a
/// byte-wise zigzag-delta residual (channel `0`). Not size-optimal, but it
/// round-trips exactly through [`decode_vertex_buffer`].
fn encode_vertex_block(
    vertices: &[u8],
    block_size: usize,
    vertex_size: usize,
    last_vertex: &mut [u8],
    out: &mut Vec<u8>,
) {
    // residuals[pos * block_size + i] = zigzag-delta byte for lane `pos`, vertex `i`.
    let mut residuals = vec![0u8; vertex_size * block_size];
    for pos in 0..vertex_size {
        let mut prev = last_vertex[pos];
        for i in 0..block_size {
            let cur = vertices[i * vertex_size + pos];
            residuals[pos * block_size + i] = zigzag8(cur.wrapping_sub(prev));
            prev = cur;
        }
    }

    // Control: one byte per 4 lanes, each lane's 2-bit nibble = 3 (literal) -> 0xFF.
    for _ in 0..(vertex_size / 4) {
        out.push(0xFF);
    }

    // Plane data, in decode consumption order: outer lane group `k` (step 4),
    // inner `j` in 0..4, plane = k + j, each `block_size` residual bytes.
    let mut k = 0usize;
    while k < vertex_size {
        for j in 0..4 {
            let pos = k + j;
            out.extend_from_slice(&residuals[pos * block_size..pos * block_size + block_size]);
        }
        k += 4;
    }

    last_vertex
        .copy_from_slice(&vertices[vertex_size * (block_size - 1)..vertex_size * block_size]);
}

/// Encodes a raw `vertex_count * vertex_size` interleaved vertex stream into a
/// meshoptimizer vertex buffer (codec v1, header `0xa1`) that
/// [`decode_vertex_buffer`] reads back byte-for-byte. This is a correctness-first
/// encoder (literal lanes, no bit-width compaction), so the output is roughly the
/// size of the uncompressed stream plus small per-block control overhead; it is
/// not byte-identical to Valve's own compressor, only round-trip equivalent under
/// the same (VRF-matched) decoder the engine uses.
pub fn encode_vertex_buffer(
    vertex_count: usize,
    vertex_size: usize,
    vertices: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    if vertex_size == 0 || vertex_size > 256 {
        return Err(DecodeError::Meshopt("vertex size out of range"));
    }
    if !vertex_size.is_multiple_of(4) {
        return Err(DecodeError::Meshopt("vertex size not a multiple of 4"));
    }
    if vertex_count == 0 {
        return Err(DecodeError::Meshopt("vertex count zero"));
    }
    if vertices.len() != vertex_count * vertex_size {
        return Err(DecodeError::Meshopt("vertex buffer length mismatch"));
    }

    let mut out = Vec::new();
    out.push(VERTEX_HEADER | DECODE_VERTEX_VERSION); // 0xa1

    // The decoder seeds the running "last vertex" from the tail and uses it as
    // the predecessor of vertex 0. Seeding with the first vertex itself makes
    // vertex 0 delta to zero, matching meshopt's convention.
    let first_vertex = vertices[..vertex_size].to_vec();
    let mut last_vertex = first_vertex.clone();

    let vertex_block_size = get_vertex_block_size(vertex_size);
    let mut vertex_offset = 0usize;
    while vertex_offset < vertex_count {
        let block_size = if vertex_offset + vertex_block_size < vertex_count {
            vertex_block_size
        } else {
            vertex_count - vertex_offset
        };
        let start = vertex_offset * vertex_size;
        encode_vertex_block(
            &vertices[start..start + block_size * vertex_size],
            block_size,
            vertex_size,
            &mut last_vertex,
            &mut out,
        );
        vertex_offset += block_size;
    }

    // Tail: optional zero padding up to the minimum, then the seed vertex, then
    // the per-lane channel descriptors (all 0 = byte-wise zigzag-delta).
    let tail_size = vertex_size + vertex_size / 4;
    let tail_size_padded = tail_size.max(TAIL_MIN_SIZE_V1);
    out.resize(out.len() + (tail_size_padded - tail_size), 0);
    out.extend_from_slice(&first_vertex);
    out.resize(out.len() + vertex_size / 4, 0);
    Ok(out)
}
