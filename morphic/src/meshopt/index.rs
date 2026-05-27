//! meshoptimizer index buffer decoder (codec v1), a port of
//! `ValveResourceFormat.Compression.MeshOptimizerIndexDecoder`, itself a port
//! of zeux's `meshoptimizer/src/indexcodec.cpp`. Source 2 `MIDX` blocks store a
//! triangle list in this format (first byte `0xe1` = index codec v1).

// Faithful port of zeux's meshoptimizer index codec (via VRF). The truncating
// index casts, the long single-pass decoder, and the fe/feb/fec/fec0 naming
// mirror the reference and are intentional, so the matching pedantic lints are
// allowed here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::similar_names
)]

use crate::error::DecodeError;

const INDEX_HEADER: u8 = 0xe0;
const DECODE_INDEX_VERSION: u8 = 1;

#[inline]
fn push_edge_fifo(fifo: &mut [(u32, u32); 16], offset: &mut usize, a: u32, b: u32) {
    fifo[*offset] = (a, b);
    *offset = (*offset + 1) & 15;
}

#[inline]
fn push_vertex_fifo(fifo: &mut [u32; 16], offset: &mut usize, v: u32, cond: bool) {
    fifo[*offset] = v;
    *offset = (*offset + usize::from(cond)) & 15;
}

fn decode_vbyte(data: &[u8], position: &mut usize) -> u32 {
    let lead = u32::from(data[*position]);
    *position += 1;
    if lead < 128 {
        return lead;
    }
    let mut result = lead & 127;
    let mut shift = 7;
    for _ in 0..4 {
        let group = u32::from(data[*position]);
        *position += 1;
        result |= (group & 127) << shift;
        shift += 7;
        if group < 128 {
            break;
        }
    }
    result
}

fn decode_index(data: &[u8], last: u32, position: &mut usize) -> u32 {
    let v = decode_vbyte(data, position);
    let d = (v >> 1) ^ 0u32.wrapping_sub(v & 1); // zigzag decode
    last.wrapping_add(d)
}

fn write_triangle(dest: &mut [u8], tri: usize, index_size: usize, a: u32, b: u32, c: u32) {
    let off = tri * index_size;
    if index_size == 2 {
        dest[off..off + 2].copy_from_slice(&(a as u16).to_le_bytes());
        dest[off + 2..off + 4].copy_from_slice(&(b as u16).to_le_bytes());
        dest[off + 4..off + 6].copy_from_slice(&(c as u16).to_le_bytes());
    } else {
        dest[off..off + 4].copy_from_slice(&a.to_le_bytes());
        dest[off + 4..off + 8].copy_from_slice(&b.to_le_bytes());
        dest[off + 8..off + 12].copy_from_slice(&c.to_le_bytes());
    }
}

/// Decodes a meshoptimizer-compressed index buffer into `index_count` indices
/// of `index_size` bytes (2 or 4) each, little-endian.
pub fn decode_index_buffer(
    index_count: usize,
    index_size: usize,
    buffer: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    if !index_count.is_multiple_of(3) {
        return Err(DecodeError::Meshopt("index count not a multiple of 3"));
    }
    if index_size != 2 && index_size != 4 {
        return Err(DecodeError::Meshopt("index size must be 2 or 4"));
    }

    let data_offset = 1 + index_count / 3;
    if buffer.len() < data_offset + 16 {
        return Err(DecodeError::Meshopt("index buffer too short"));
    }
    if buffer[0] & 0xF0 != INDEX_HEADER {
        return Err(DecodeError::Meshopt("bad index buffer header"));
    }
    if buffer[0] & 0x0F > DECODE_INDEX_VERSION {
        return Err(DecodeError::Meshopt("unsupported index codec version"));
    }
    let version = u32::from(buffer[0] & 0x0F);

    let mut vertex_fifo = [0u32; 16];
    let mut edge_fifo = [(0u32, 0u32); 16];
    let mut edge_off = 0usize;
    let mut vert_off = 0usize;
    let mut next = 0u32;
    let mut last = 0u32;
    let fecmax: u32 = if version >= 1 { 13 } else { 15 };

    let mut buffer_index = 1usize;
    let data = &buffer[data_offset..buffer.len() - 16];
    let codeaux_table = &buffer[buffer.len() - 16..];

    let mut dest = vec![0u8; index_count * index_size];
    let mut position = 0usize;

    let mut i = 0usize;
    while i < index_count {
        let codetri = buffer[buffer_index];
        buffer_index += 1;

        if codetri < 0xf0 {
            let fe = (codetri >> 4) as usize;
            let (a, b) = edge_fifo[(edge_off.wrapping_sub(1).wrapping_sub(fe)) & 15];

            let fec = u32::from(codetri & 15);
            let c;
            if fec < fecmax {
                c = if fec == 0 {
                    next
                } else {
                    vertex_fifo[(vert_off.wrapping_sub(1).wrapping_sub(fec as usize)) & 15]
                };
                let fec0 = fec == 0;
                if fec0 {
                    next += 1;
                }
                push_vertex_fifo(&mut vertex_fifo, &mut vert_off, c, fec0);
            } else {
                last = if fec == 15 {
                    decode_index(data, last, &mut position)
                } else {
                    last.wrapping_add(fec.wrapping_sub(fec ^ 3))
                };
                c = last;
                push_vertex_fifo(&mut vertex_fifo, &mut vert_off, c, true);
            }

            push_edge_fifo(&mut edge_fifo, &mut edge_off, c, b);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, a, c);
            write_triangle(&mut dest, i, index_size, a, b, c);
        } else if codetri < 0xfe {
            let codeaux = codeaux_table[(codetri & 15) as usize];
            let feb = (codeaux >> 4) as usize;
            let fec = (codeaux & 15) as usize;

            let a = next;
            next += 1;
            let b = if feb == 0 {
                next
            } else {
                vertex_fifo[(vert_off.wrapping_sub(feb)) & 15]
            };
            let feb0 = feb == 0;
            if feb0 {
                next += 1;
            }
            let c = if fec == 0 {
                next
            } else {
                vertex_fifo[(vert_off.wrapping_sub(fec)) & 15]
            };
            let fec0 = fec == 0;
            if fec0 {
                next += 1;
            }

            write_triangle(&mut dest, i, index_size, a, b, c);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, a, true);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, b, feb0);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, c, fec0);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, b, a);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, c, b);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, a, c);
        } else {
            let codeaux = u32::from(data[position]);
            position += 1;

            let fea = if codetri == 0xfe { 0u32 } else { 15u32 };
            let feb = codeaux >> 4;
            let fec = codeaux & 15;

            if codeaux == 0 {
                next = 0;
            }

            let mut a = if fea == 0 {
                let v = next;
                next += 1;
                v
            } else {
                0
            };
            let mut b = if feb == 0 {
                let v = next;
                next += 1;
                v
            } else {
                vertex_fifo[(vert_off.wrapping_sub(feb as usize)) & 15]
            };
            let mut c = if fec == 0 {
                let v = next;
                next += 1;
                v
            } else {
                vertex_fifo[(vert_off.wrapping_sub(fec as usize)) & 15]
            };

            if fea == 15 {
                last = decode_index(data, last, &mut position);
                a = last;
            }
            if feb == 15 {
                last = decode_index(data, last, &mut position);
                b = last;
            }
            if fec == 15 {
                last = decode_index(data, last, &mut position);
                c = last;
            }

            write_triangle(&mut dest, i, index_size, a, b, c);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, a, true);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, b, feb == 0 || feb == 15);
            push_vertex_fifo(&mut vertex_fifo, &mut vert_off, c, fec == 0 || fec == 15);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, b, a);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, c, b);
            push_edge_fifo(&mut edge_fifo, &mut edge_off, a, c);
        }

        i += 3;
    }

    if position != data.len() {
        return Err(DecodeError::Meshopt(
            "index decode did not consume all data",
        ));
    }
    Ok(dest)
}
