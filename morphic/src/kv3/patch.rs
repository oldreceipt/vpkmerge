//! Surgical, byte-faithful KV3 edits.
//!
//! Re-encoding a model `MDAT` from the decoded [`Value`](super::Value) tree is too
//! lossy for the engine (it drops value flags and auxiliary-buffer typed-array
//! tags; see [`super::rewrap`]). For draw-call removal we therefore do **not**
//! re-encode: we [`rewrap_uncompressed`] the block (preserving every structural
//! byte) and then zero a few scalar fields *in place*. A draw call whose
//! `m_nIndexCount` is 0 submits no primitives, so the part stops rendering while
//! the block stays byte-identical everywhere else.
//!
//! [`neutralize_draw_calls`] walks the KV3 value tree exactly as the reader does
//! (same lane/cursor discipline), but tracking each scalar's absolute byte offset
//! in the block instead of building a tree, so it can locate and zero the
//! `m_nIndexCount` of specific `m_sceneObjects[so].m_drawCalls[dc]` entries. Only
//! the v5 two-buffer layout (what Deadlock models use) is handled.

// The KV3 header stores its counts/sizes as i32; reading them as usize is the
// same intended reinterpretation the reader makes (counts are never negative).
#![allow(clippy::cast_sign_loss)]

use super::node;
use super::rewrap::rewrap_uncompressed;
use crate::error::DecodeError;

const B1: usize = 0;
const B2: usize = 1;
const B4: usize = 2;
const B8: usize = 3;

/// Zeroes the `m_nIndexCount` of each `(scene_object, draw_call)` in `targets`,
/// so those draw calls render nothing, returning the edited (uncompressed) block.
/// The input may be compressed; it is re-wrapped uncompressed first. Every byte
/// other than the targeted counts is preserved (flags, typed arrays, structure),
/// which is what the engine's model loader requires.
///
/// Errors if the block is not v5 (the model `MDAT` layout) or if a target's
/// `m_nIndexCount` is stored as a tagless zero/one constant (never the case for a
/// real, non-empty draw call).
pub fn neutralize_draw_calls(
    block: &[u8],
    targets: &[(usize, usize)],
) -> Result<Vec<u8>, DecodeError> {
    let mut out = rewrap_uncompressed(block)?;
    if out.len() < 120 || u32::from_le_bytes([out[0], out[1], out[2], out[3]]) & 0xFF != 5 {
        return Err(DecodeError::Kv3("draw-call neutralize requires KV3 v5"));
    }

    let patches = {
        let mut w = Walk::new(&out, targets)?;
        // The root value's type byte leads the type stream (the reader consumes it
        // before reading the root); consume it too, then walk the root value.
        let root = w.read_type()?;
        w.value(root, Where::Root)?;
        w.patches
    };
    if patches.is_empty() {
        return Err(DecodeError::Model(
            "no targeted draw call had a patchable m_nIndexCount",
        ));
    }
    for (off, width) in patches {
        for b in out.get_mut(off..off + width).ok_or(DecodeError::Kv3(
            "patch offset out of range (walker/layout mismatch)",
        ))? {
            *b = 0;
        }
    }
    Ok(out)
}

/// Where in the target path the walker currently is. Only the path to
/// `m_sceneObjects[*].m_drawCalls[*].m_nIndexCount` is tracked; everything else is
/// [`Where::Other`] and merely skipped (advancing cursors correctly).
#[derive(Clone, Copy)]
enum Where {
    Root,
    InSceneObjects,
    InSceneObject(usize),
    InDrawCalls(usize),
    InDrawCall(usize, usize),
    /// This scalar is a targeted `m_nIndexCount`; record its byte offset.
    Record,
    Other,
}

/// One typed lane (b1/b2/b4/b8): an absolute base in the block plus a moving
/// cursor. `at()` is the absolute offset of the next unread byte.
#[derive(Clone, Copy)]
struct Lane {
    base: usize,
    pos: usize,
}

impl Lane {
    fn at(&self) -> usize {
        self.base + self.pos
    }
}

struct Walk<'a> {
    block: &'a [u8],
    types: Lane,
    obj_lengths: Lane,
    main: [Lane; 4],
    aux: [Lane; 4],
    strings: Vec<String>,
    targets: &'a [(usize, usize)],
    patches: Vec<(usize, usize)>,
}

impl<'a> Walk<'a> {
    fn new(block: &'a [u8], targets: &'a [(usize, usize)]) -> Result<Self, DecodeError> {
        // Header counts (v5). Aux counts are the "first" count block; main counts
        // sit in the v5-specific tail. Mirrors `reader::decode`'s field offsets.
        let aux_b1 = i32_at(block, 28)? as usize;
        let aux_b4 = i32_at(block, 32)? as usize;
        let aux_b8 = i32_at(block, 36)? as usize;
        let aux_b2 = i32_at(block, 64)? as usize;
        let unc_buf1 = i32_at(block, 72)? as usize;
        let main_b1 = i32_at(block, 88)? as usize;
        let main_b2 = i32_at(block, 92)? as usize;
        let main_b4 = i32_at(block, 96)? as usize;
        let main_b8 = i32_at(block, 100)? as usize;
        let main_obj = i32_at(block, 108)? as usize;

        let buf1 = 120usize;
        let buf2 = buf1 + unc_buf1;

        // Aux buffer: [strings... in b1][b2][b4][b8]; string count is the first
        // int of aux b4, and aux b4's value region begins after it.
        let mut off = 0usize;
        let a_b1_start = off;
        off += aux_b1;
        let (a_b2_start, _) = lane(&mut off, aux_b2, 2);
        let (a_b4_start, _) = lane(&mut off, aux_b4, 4);
        let (a_b8_start, _) = lane(&mut off, aux_b8, 8);
        let string_count = u32::try_from(i32_at(block, buf1 + a_b4_start)?)
            .map_err(|_| DecodeError::Kv3("negative string count"))?
            as usize;
        let mut sp = buf1 + a_b1_start;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            strings.push(read_cstr(block, &mut sp)?);
        }
        let aux = [
            Lane { base: sp, pos: 0 },
            Lane {
                base: buf1 + a_b2_start,
                pos: 0,
            },
            Lane {
                base: buf1 + a_b4_start + 4,
                pos: 0,
            },
            Lane {
                base: buf1 + a_b8_start,
                pos: 0,
            },
        ];

        // Main buffer: [object_lengths][b1][b2][b4][b8][types].
        let mut off = 0usize;
        let ol_start = off;
        off += main_obj * 4;
        let m_b1_start = off;
        off += main_b1;
        let (m_b2_start, _) = lane(&mut off, main_b2, 2);
        let (m_b4_start, _) = lane(&mut off, main_b4, 4);
        let (m_b8_start, _) = lane(&mut off, main_b8, 8);
        let types_start = off;

        let main = [
            Lane {
                base: buf2 + m_b1_start,
                pos: 0,
            },
            Lane {
                base: buf2 + m_b2_start,
                pos: 0,
            },
            Lane {
                base: buf2 + m_b4_start,
                pos: 0,
            },
            Lane {
                base: buf2 + m_b8_start,
                pos: 0,
            },
        ];

        Ok(Walk {
            block,
            types: Lane {
                base: buf2 + types_start,
                pos: 0,
            },
            obj_lengths: Lane {
                base: buf2 + ol_start,
                pos: 0,
            },
            main,
            aux,
            strings,
            targets,
            patches: Vec::new(),
        })
    }

    fn read_type(&mut self) -> Result<u8, DecodeError> {
        let mut t = *self
            .block
            .get(self.types.at())
            .ok_or(DecodeError::Kv3("type stream underrun"))?;
        self.types.pos += 1;
        if t & 0x80 != 0 {
            t &= 0x3F; // v5 masks 0x3F; the flag byte follows and is skipped.
            self.types.pos += 1;
        }
        Ok(t)
    }

    fn lane_u32(&mut self, lane: usize) -> Result<u32, DecodeError> {
        let at = self.main[lane].at();
        let v = u32_at(self.block, at)?;
        self.main[lane].pos += 4;
        Ok(v)
    }

    fn lane_u8(&mut self, lane: usize) -> Result<u8, DecodeError> {
        let v = *self
            .block
            .get(self.main[lane].at())
            .ok_or(DecodeError::Kv3("lane underrun"))?;
        self.main[lane].pos += 1;
        Ok(v)
    }

    fn obj_len(&mut self) -> Result<u32, DecodeError> {
        let v = u32_at(self.block, self.obj_lengths.at())?;
        self.obj_lengths.pos += 4;
        Ok(v)
    }

    fn key(&self, id: u32) -> &str {
        if id == u32::MAX {
            ""
        } else {
            self.strings.get(id as usize).map_or("", String::as_str)
        }
    }

    /// Walks one value of type `datatype`, advancing every cursor exactly as the
    /// reader would, and recording the byte offset when `where_` is [`Where::Record`].
    #[allow(clippy::wildcard_imports)] // node constants, mirroring reader::read_value
    fn value(&mut self, datatype: u8, where_: Where) -> Result<(), DecodeError> {
        use node::*;
        let record = matches!(where_, Where::Record);
        match datatype {
            // Lane-backed scalars: record before advancing if this is a target.
            INT32 | UINT32 | FLOAT => {
                if record && datatype != FLOAT {
                    self.patches.push((self.main[B4].at(), 4));
                }
                self.main[B4].pos += 4;
            }
            INT64 | UINT64 | DOUBLE => {
                if record && datatype != DOUBLE {
                    self.patches.push((self.main[B8].at(), 8));
                }
                self.main[B8].pos += 8;
            }
            INT16 | UINT16 => {
                if record {
                    self.patches.push((self.main[B2].at(), 2));
                }
                self.main[B2].pos += 2;
            }
            INT32_AS_BYTE | BOOLEAN => {
                if record && datatype == INT32_AS_BYTE {
                    self.patches.push((self.main[B1].at(), 1));
                }
                self.main[B1].pos += 1;
            }
            STRING => {
                self.main[B4].pos += 4;
            }
            NULL | BOOLEAN_TRUE | BOOLEAN_FALSE | INT64_ZERO | INT64_ONE | DOUBLE_ZERO
            | DOUBLE_ONE => {}
            ARRAY => {
                let n = self.lane_u32(B4)?;
                for i in 0..n {
                    let t = self.read_type()?;
                    self.value(t, child_index(where_, i as usize))?;
                }
            }
            ARRAY_TYPED => {
                let n = self.lane_u32(B4)?;
                let sub = self.read_type()?;
                for i in 0..n {
                    self.value(sub, child_index(where_, i as usize))?;
                }
            }
            ARRAY_TYPE_BYTE_LENGTH => {
                let n = u32::from(self.lane_u8(B1)?);
                let sub = self.read_type()?;
                for i in 0..n {
                    self.value(sub, child_index(where_, i as usize))?;
                }
            }
            ARRAY_TYPE_AUXILIARY_BUFFER => {
                let n = u32::from(self.lane_u8(B1)?);
                let sub = self.read_type()?;
                std::mem::swap(&mut self.main, &mut self.aux);
                for i in 0..n {
                    self.value(sub, child_index(where_, i as usize))?;
                }
                std::mem::swap(&mut self.main, &mut self.aux);
            }
            OBJECT => {
                let n = self.obj_len()?;
                for _ in 0..n {
                    let vt = self.read_type()?;
                    let id = self.lane_u32(B4)?;
                    let child = child_key(where_, self.key(id), self.targets);
                    self.value(vt, child)?;
                }
            }
            other => return Err(DecodeError::Kv3NodeType(other)),
        }
        Ok(())
    }
}

/// Refines the path state when descending into object member `key`.
fn child_key(where_: Where, key: &str, targets: &[(usize, usize)]) -> Where {
    match where_ {
        Where::Root if key == "m_sceneObjects" => Where::InSceneObjects,
        Where::InSceneObject(so) if key == "m_drawCalls" => Where::InDrawCalls(so),
        Where::InDrawCall(so, dc) if key == "m_nIndexCount" && targets.contains(&(so, dc)) => {
            Where::Record
        }
        _ => Where::Other,
    }
}

/// Refines the path state when descending into array element `i`.
fn child_index(where_: Where, i: usize) -> Where {
    match where_ {
        Where::InSceneObjects => Where::InSceneObject(i),
        Where::InDrawCalls(so) => Where::InDrawCall(so, i),
        _ => Where::Other,
    }
}

fn align(off: &mut usize, a: usize) {
    *off = (*off + (a - 1)) & !(a - 1);
}

fn lane(off: &mut usize, count: usize, elem: usize) -> (usize, usize) {
    if count == 0 {
        return (*off, 0);
    }
    align(off, elem);
    let start = *off;
    let len = count * elem;
    *off += len;
    (start, len)
}

fn i32_at(b: &[u8], o: usize) -> Result<i32, DecodeError> {
    let s = b
        .get(o..o + 4)
        .ok_or(DecodeError::Kv3("header field out of range"))?;
    Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, DecodeError> {
    let s = b
        .get(o..o + 4)
        .ok_or(DecodeError::Kv3("lane read out of range"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_cstr(buf: &[u8], pos: &mut usize) -> Result<String, DecodeError> {
    let start = *pos;
    while *pos < buf.len() && buf[*pos] != 0 {
        *pos += 1;
    }
    if *pos >= buf.len() {
        return Err(DecodeError::Kv3("unterminated string"));
    }
    let s = String::from_utf8_lossy(&buf[start..*pos]).into_owned();
    *pos += 1;
    Ok(s)
}
