//! Binary KV3 reader, ported from `ValveResourceFormat`'s `BinaryKV3.cs`
//! (the unified `ReadBuffer` path) and `BinaryKV3.NodeType.cs`.
//!
//! morphic only needs the value tree, so KV3 *flags* (resource / panorama /
//! soundevent / ...) are consumed from the type-tag stream for alignment but
//! discarded: they annotate a value, they don't change it.
//!
//! Deadlock hero models pin **KV3 v5** (magic `0x4B563305`) with LZ4
//! (compression method 1). v5 splits the payload into two buffers, each a
//! single raw LZ4 block (the 16384-byte frame size only chunks the optional
//! binary-blob section, which these blocks don't use):
//!
//! - **buffer 1 / auxiliary**: its `bytes4` region opens with the string count,
//!   its `bytes1` region holds the null-terminated string table; the remainder
//!   feeds homogeneous typed arrays (see `ARRAY_TYPE_AUXILIARY_BUFFER`).
//! - **buffer 2 / main**: leads with the object-length table, then the
//!   1/2/4/8-byte scalar pools, then the type-tag stream, then a `0xFFEEDD00`
//!   trailer (all inside the decompressed buffer when there are no blobs).
//!
//! Only v5 + methods 0 (none) and 1 (LZ4) are implemented; other versions and
//! ZSTD return a clear error rather than guessing.

use std::collections::BTreeMap;

use super::types::Value;
use crate::error::DecodeError;

const MAGIC_V0: u32 = 0x0356_4B56; // "VKV\x03" (pre-versioned)
const MAGIC_BASE: u32 = 0x4B56_3300; // "..3VK" with the version in the low byte
const TRAILER: u32 = 0xFFEE_DD00;
const LZ4_FRAME_SIZE: u16 = 16384;

/// KV3 binary node-type tags (`BinaryKV3.NodeType.cs`).
mod tag {
    pub const NULL: u8 = 1;
    pub const BOOLEAN: u8 = 2;
    pub const INT64: u8 = 3;
    pub const UINT64: u8 = 4;
    pub const DOUBLE: u8 = 5;
    pub const STRING: u8 = 6;
    pub const BINARY_BLOB: u8 = 7;
    pub const ARRAY: u8 = 8;
    pub const OBJECT: u8 = 9;
    pub const ARRAY_TYPED: u8 = 10;
    pub const INT32: u8 = 11;
    pub const UINT32: u8 = 12;
    pub const BOOLEAN_TRUE: u8 = 13;
    pub const BOOLEAN_FALSE: u8 = 14;
    pub const INT64_ZERO: u8 = 15;
    pub const INT64_ONE: u8 = 16;
    pub const DOUBLE_ZERO: u8 = 17;
    pub const DOUBLE_ONE: u8 = 18;
    pub const FLOAT: u8 = 19;
    pub const INT16: u8 = 20;
    pub const UINT16: u8 = 21;
    pub const INT32_AS_BYTE: u8 = 23;
    pub const ARRAY_TYPE_BYTE_LENGTH: u8 = 24;
    pub const ARRAY_TYPE_AUXILIARY_BUFFER: u8 = 25;
}

/// Reads a binary KV3 document (a self-contained block) into a [`Value`] tree.
pub fn parse(data: &[u8]) -> Result<Value, DecodeError> {
    let mut c = Cursor::new(data);
    let magic = c.u32()?;

    if magic == MAGIC_V0 {
        return Err(DecodeError::UnsupportedKv3(magic));
    }
    if magic & 0xFFFF_FF00 != MAGIC_BASE {
        return Err(DecodeError::UnsupportedKv3(magic));
    }
    let version = magic & 0xFF;
    if version != 5 {
        // v1..v4 share most of this code but lay strings/types out differently;
        // implement + fixture-validate them when a block actually needs one.
        return Err(DecodeError::UnsupportedKv3(magic));
    }

    read_v5(&mut c)
}

fn read_v5(c: &mut Cursor) -> Result<Value, DecodeError> {
    let _format_guid = c.take(16)?;
    let compression_method = c.u32()?;
    let compression_dictionary_id = c.u16()?;
    let compression_frame_size = c.u16()?;

    // Buffer-1 (auxiliary) scalar-pool counts + global counts.
    let count_bytes1 = c.u32()? as usize;
    let count_bytes4 = c.u32()? as usize;
    let count_bytes8 = c.u32()? as usize;
    let count_types = c.u32()? as usize;
    let _count_objects = c.u16()?;
    let _count_arrays = c.u16()?;
    let _size_uncompressed_total = c.u32()? as usize;
    let _size_compressed_total = c.u32()? as usize;
    let count_blocks = c.u32()? as usize;
    let _size_binary_blobs = c.u32()? as usize;

    // version >= 4
    let count_bytes2 = c.u32()? as usize;
    let _size_block_compressed_sizes = c.u32()? as usize;

    // version >= 5
    let size_uncompressed_buffer1 = c.u32()? as usize;
    let size_compressed_buffer1 = c.u32()? as usize;
    let size_uncompressed_buffer2 = c.u32()? as usize;
    let size_compressed_buffer2 = c.u32()? as usize;
    let count_bytes1_b2 = c.u32()? as usize;
    let count_bytes2_b2 = c.u32()? as usize;
    let count_bytes4_b2 = c.u32()? as usize;
    let count_bytes8_b2 = c.u32()? as usize;
    let _unk13 = c.u32()?;
    let count_objects_b2 = c.u32()? as usize;
    let _count_arrays_b2 = c.u32()?;
    let _unk16 = c.u32()?;

    if compression_dictionary_id != 0 {
        return Err(DecodeError::Kv3("KV3 compression dictionary unsupported"));
    }
    if compression_method == 1 && compression_frame_size != LZ4_FRAME_SIZE {
        return Err(DecodeError::Kv3("KV3 unexpected LZ4 frame size"));
    }
    if count_blocks > 0 {
        // Binary blobs use chained (sliding-dictionary) LZ4 frames; no Deadlock
        // model/material KV3 block ships one, so this is unimplemented for now.
        return Err(DecodeError::Kv3("KV3 binary-blob section not supported"));
    }

    let buffer1 = read_block(
        c,
        compression_method,
        size_uncompressed_buffer1,
        size_compressed_buffer1,
    )?;
    let buffer2 = read_block(
        c,
        compression_method,
        size_uncompressed_buffer2,
        size_compressed_buffer2,
    )?;

    // --- Carve buffer 1 (auxiliary): scalar pools, then string table. ---
    let mut off = 0usize;
    let r_b1 = carve(&buffer1, &mut off, count_bytes1, 1)?;
    let r_b2 = carve(&buffer1, &mut off, count_bytes2, 2)?;
    let r_b4 = carve(&buffer1, &mut off, count_bytes4, 4)?;
    let r_b8 = carve(&buffer1, &mut off, count_bytes8, 8)?;
    let mut aux = Buffers {
        bytes1: r_b1,
        bytes2: r_b2,
        bytes4: r_b4,
        bytes8: r_b8,
    };
    if aux.bytes4.remaining() < 4 {
        return Err(DecodeError::Kv3("KV3 missing string count"));
    }
    let count_strings = aux.bytes4.u32()? as usize;
    let mut strings = Vec::with_capacity(count_strings);
    for _ in 0..count_strings {
        strings.push(read_nullterm_utf8(&mut aux.bytes1)?);
    }

    // --- Carve buffer 2 (main): object lengths, scalar pools, type stream. ---
    let mut off = count_objects_b2
        .checked_mul(4)
        .ok_or(DecodeError::Kv3("KV3 object-length table overflow"))?;
    let object_lengths = slice_reader(&buffer2, 0, off)?;
    let m_b1 = carve(&buffer2, &mut off, count_bytes1_b2, 1)?;
    let m_b2 = carve(&buffer2, &mut off, count_bytes2_b2, 2)?;
    let m_b4 = carve(&buffer2, &mut off, count_bytes4_b2, 4)?;
    let m_b8 = carve(&buffer2, &mut off, count_bytes8_b2, 8)?;
    let types = slice_reader(&buffer2, off, count_types)?;
    off += count_types;

    let trailer = read_u32_at(&buffer2, off)?;
    if trailer != TRAILER {
        return Err(DecodeError::Kv3("bad KV3 trailer"));
    }

    let mut ctx = Context {
        types,
        object_lengths,
        strings,
        buffer: Buffers {
            bytes1: m_b1,
            bytes2: m_b2,
            bytes4: m_b4,
            bytes8: m_b8,
        },
        aux,
    };

    let root_type = read_type(&mut ctx)?;
    let root = read_value(&mut ctx, root_type)?;

    // The type-tag stream is the spine of the document; a leftover tag means we
    // mis-parsed a value's width somewhere.
    if ctx.types.remaining() != 0 {
        return Err(DecodeError::Kv3("KV3 type stream not fully consumed"));
    }
    Ok(root)
}

/// Reads one payload buffer from the stream and returns it uncompressed.
fn read_block(
    c: &mut Cursor,
    method: u32,
    size_uncompressed: usize,
    size_compressed: usize,
) -> Result<Vec<u8>, DecodeError> {
    match method {
        0 => Ok(c.take(size_uncompressed)?.to_vec()),
        1 => {
            let input = c.take(size_compressed)?;
            let mut out = vec![0u8; size_uncompressed];
            let written = lz4_flex::block::decompress_into(input, &mut out)
                .map_err(|_| DecodeError::Kv3("KV3 LZ4 decompress failed"))?;
            if written != size_uncompressed {
                return Err(DecodeError::Kv3("KV3 LZ4 size mismatch"));
            }
            Ok(out)
        }
        2 => Err(DecodeError::Kv3("KV3 ZSTD compression not supported")),
        _ => Err(DecodeError::Kv3("unknown KV3 compression method")),
    }
}

/// Per-buffer scalar pools. Reads of a given width draw from the matching pool.
struct Buffers {
    bytes1: Reader,
    bytes2: Reader,
    bytes4: Reader,
    bytes8: Reader,
}

struct Context {
    types: Reader,
    object_lengths: Reader,
    strings: Vec<String>,
    buffer: Buffers,
    aux: Buffers,
}

/// Reads the next node type tag, consuming (and discarding) a trailing flag
/// byte when the high bit is set, per the v3+ encoding.
fn read_type(ctx: &mut Context) -> Result<u8, DecodeError> {
    let mut databyte = ctx.types.u8()?;
    if databyte & 0x80 != 0 {
        databyte &= 0x3F;
        let _flag = ctx.types.u8()?; // value metadata only; irrelevant to the tree
    }
    Ok(databyte)
}

fn read_value(ctx: &mut Context, datatype: u8) -> Result<Value, DecodeError> {
    match datatype {
        tag::NULL => Ok(Value::Null),
        tag::BOOLEAN_TRUE => Ok(Value::Bool(true)),
        tag::BOOLEAN_FALSE => Ok(Value::Bool(false)),
        tag::INT64_ZERO => Ok(Value::Int(0)),
        tag::INT64_ONE => Ok(Value::Int(1)),
        tag::DOUBLE_ZERO => Ok(Value::Double(0.0)),
        tag::DOUBLE_ONE => Ok(Value::Double(1.0)),

        tag::BOOLEAN => Ok(Value::Bool(ctx.buffer.bytes1.u8()? == 1)),
        tag::INT32_AS_BYTE => Ok(Value::Int(i64::from(ctx.buffer.bytes1.u8()?))),
        tag::INT16 => Ok(Value::Int(i64::from(ctx.buffer.bytes2.i16()?))),
        tag::UINT16 => Ok(Value::UInt(u64::from(ctx.buffer.bytes2.u16()?))),
        tag::INT32 => Ok(Value::Int(i64::from(ctx.buffer.bytes4.i32()?))),
        tag::UINT32 => Ok(Value::UInt(u64::from(ctx.buffer.bytes4.u32()?))),
        tag::FLOAT => Ok(Value::Double(f64::from(ctx.buffer.bytes4.f32()?))),
        tag::INT64 => Ok(Value::Int(ctx.buffer.bytes8.i64()?)),
        tag::UINT64 => Ok(Value::UInt(ctx.buffer.bytes8.u64()?)),
        tag::DOUBLE => Ok(Value::Double(ctx.buffer.bytes8.f64()?)),

        tag::STRING => {
            let id = ctx.buffer.bytes4.i32()?;
            Ok(Value::String(string_by_id(ctx, id)?))
        }

        tag::ARRAY => {
            let n = ctx.buffer.bytes4.u32()? as usize;
            let mut items = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                let t = read_type(ctx)?;
                items.push(read_value(ctx, t)?);
            }
            Ok(Value::Array(items))
        }

        tag::ARRAY_TYPED | tag::ARRAY_TYPE_BYTE_LENGTH => {
            let n = if datatype == tag::ARRAY_TYPE_BYTE_LENGTH {
                usize::from(ctx.buffer.bytes1.u8()?)
            } else {
                ctx.buffer.bytes4.u32()? as usize
            };
            let sub = read_type(ctx)?;
            let mut items = Vec::with_capacity(n.min(1 << 16));
            for _ in 0..n {
                items.push(read_value(ctx, sub)?);
            }
            Ok(Value::Array(items))
        }

        tag::ARRAY_TYPE_AUXILIARY_BUFFER => {
            let n = usize::from(ctx.buffer.bytes1.u8()?);
            let sub = read_type(ctx)?;
            // Homogeneous elements are packed in the auxiliary buffer: swap it in
            // as the active scalar source, read, then swap back. The type and
            // object-length streams are shared and stay put.
            std::mem::swap(&mut ctx.buffer, &mut ctx.aux);
            let mut items = Vec::with_capacity(n);
            let mut err = None;
            for _ in 0..n {
                match read_value(ctx, sub) {
                    Ok(v) => items.push(v),
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            }
            std::mem::swap(&mut ctx.buffer, &mut ctx.aux);
            match err {
                Some(e) => Err(e),
                None => Ok(Value::Array(items)),
            }
        }

        tag::OBJECT => {
            let n = ctx.object_lengths.u32()? as usize;
            let mut map = BTreeMap::new();
            for _ in 0..n {
                let t = read_type(ctx)?;
                let id = ctx.buffer.bytes4.i32()?;
                let name = string_by_id(ctx, id)?;
                map.insert(name, read_value(ctx, t)?);
            }
            Ok(Value::Object(map))
        }

        tag::BINARY_BLOB => Err(DecodeError::Kv3("KV3 BINARY_BLOB without block section")),
        _ => Err(DecodeError::Kv3("unknown KV3 node type")),
    }
}

fn string_by_id(ctx: &Context, id: i32) -> Result<String, DecodeError> {
    if id == -1 {
        return Ok(String::new());
    }
    let idx = usize::try_from(id).map_err(|_| DecodeError::Kv3("negative KV3 string id"))?;
    ctx.strings
        .get(idx)
        .cloned()
        .ok_or(DecodeError::Kv3("KV3 string id out of range"))
}

fn read_nullterm_utf8(r: &mut Reader) -> Result<String, DecodeError> {
    let rest = &r.data[r.pos..];
    let nul = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(DecodeError::Kv3("unterminated KV3 string"))?;
    let s = String::from_utf8_lossy(&rest[..nul]).into_owned();
    r.pos += nul + 1;
    Ok(s)
}

// --- buffer carving helpers ---

#[inline]
fn align(off: usize, alignment: usize) -> usize {
    (off + alignment - 1) & !(alignment - 1)
}

/// Carves the next `count`-element pool (each element `width` bytes) out of
/// `buf`, advancing `off` past it. Empty pools consume nothing. Mirrors the
/// align-then-slice sequence VRF uses per buffer.
fn carve(buf: &[u8], off: &mut usize, count: usize, width: usize) -> Result<Reader, DecodeError> {
    if count == 0 {
        return Ok(Reader::empty());
    }
    if width > 1 {
        *off = align(*off, width);
    }
    let len = count
        .checked_mul(width)
        .ok_or(DecodeError::Kv3("KV3 pool size overflow"))?;
    let r = slice_reader(buf, *off, len)?;
    *off += len;
    Ok(r)
}

fn slice_reader(buf: &[u8], start: usize, len: usize) -> Result<Reader, DecodeError> {
    let end = start
        .checked_add(len)
        .ok_or(DecodeError::Kv3("KV3 buffer slice overflow"))?;
    if end > buf.len() {
        return Err(DecodeError::Truncated {
            offset: start as u64,
            needed: len,
            had: buf.len().saturating_sub(start),
        });
    }
    Ok(Reader::new(buf[start..end].to_vec()))
}

fn read_u32_at(buf: &[u8], at: usize) -> Result<u32, DecodeError> {
    let end = at
        .checked_add(4)
        .ok_or(DecodeError::Kv3("KV3 trailer offset overflow"))?;
    if end > buf.len() {
        return Err(DecodeError::Truncated {
            offset: at as u64,
            needed: 4,
            had: buf.len().saturating_sub(at),
        });
    }
    Ok(u32::from_le_bytes(buf[at..end].try_into().unwrap()))
}

// --- little readers ---

/// Position-tracking reader over the input stream.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(DecodeError::Kv3("KV3 read offset overflow"))?;
        if end > self.data.len() {
            return Err(DecodeError::Truncated {
                offset: self.pos as u64,
                needed: n,
                had: self.data.len().saturating_sub(self.pos),
            });
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Position-tracking reader over an owned (decompressed) scalar pool.
struct Reader {
    data: Vec<u8>,
    pos: usize,
}

impl Reader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }

    fn empty() -> Self {
        Self {
            data: Vec::new(),
            pos: 0,
        }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&[u8], DecodeError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(DecodeError::Kv3("KV3 pool offset overflow"))?;
        if end > self.data.len() {
            return Err(DecodeError::Kv3("KV3 scalar-pool underrun"));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}
