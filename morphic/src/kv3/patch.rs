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
use super::rewrap::{
    decompress_v5_working, is_blobbed_lz4_v5, reassemble_blobbed_v5, rewrap_uncompressed,
};
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

/// One step of a KV3 path: an object member key or an array element index.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Seg {
    /// Descend into the object member with this name.
    Key(String),
    /// Descend into this array element.
    Index(usize),
}

/// Sets integer scalar fields located by KV3 path, in place, on a byte-faithful
/// uncompressed re-wrap of `block` (so value flags + typed arrays are preserved,
/// as the engine's model loader requires). Each edit is a `(path, value)`: the
/// path must resolve to exactly one settable integer scalar (`INT32`/`UINT32`/
/// `INT16`/`UINT16`/`INT32_AS_BYTE` and their 64-bit forms), and `value` must fit
/// that field's existing on-disk width.
///
/// This is the additive cousin of [`neutralize_draw_calls`] (which only zeroes):
/// it can rewrite a buffer's element count / stride, a layout field's format /
/// offset, or a draw call's vertex/index counts when replacing a mesh part in
/// place. It does **not** change a field's storage width: if the new value does
/// not fit the original encoding (e.g. a byte-stored value growing past 255),
/// it errors rather than corrupt the block (that needs a structural re-encode).
///
/// Errors if the block is not v4/v5, if any path is missing / not an integer
/// scalar / ambiguous, or if a value does not fit its field's width.
pub fn set_scalars(block: &[u8], edits: &[(Vec<Seg>, i64)]) -> Result<Vec<u8>, DecodeError> {
    let mut out = rewrap_uncompressed(block)?;
    // v5 uses a two-buffer layout (120-byte header); v4 a single buffer (72-byte
    // header). Both patch in place once decompressed; only the lane math differs.
    let version = u32_at(&out, 0)? & 0xFF;
    let min_header = if version == 5 { 120 } else { 72 };
    if out.len() < min_header || (version != 4 && version != 5) {
        return Err(DecodeError::Kv3("scalar patch requires KV3 v4 or v5"));
    }

    let targets: Vec<&[Seg]> = edits.iter().map(|(p, _)| p.as_slice()).collect();
    let hits = {
        let mut w = PathWalk::new(&out, &targets)?;
        let root = w.read_type()?;
        w.value(root)?;
        w.hits
    };

    // Every edit must resolve to exactly one settable integer scalar.
    for i in 0..edits.len() {
        match hits.iter().filter(|h| h.edit == i).count() {
            0 => {
                return Err(DecodeError::Kv3(
                    "scalar patch path not found or not an integer scalar",
                ))
            }
            1 => {}
            _ => {
                return Err(DecodeError::Kv3(
                    "scalar patch path is ambiguous (matched more than one field)",
                ))
            }
        }
    }

    for h in &hits {
        let bytes = fit_scalar(edits[h.edit].1, h.datatype)?;
        out.get_mut(h.offset..h.offset + bytes.len())
            .ok_or(DecodeError::Kv3("scalar patch offset out of range"))?
            .copy_from_slice(&bytes);
    }
    Ok(out)
}

/// Sets `DOUBLE` (f64) fields located by KV3 path, in place, on a byte-faithful
/// uncompressed re-wrap of `block`. The double sibling of [`set_scalars`]: built to
/// retint a material's `g_vColorTint` vector (an array of f64 RGBA in a `.vmat_c`)
/// without re-encoding, so the rest of the material (textures, flags, shader) stays
/// byte-identical. Each edit's path must resolve to exactly one **real** `DOUBLE`
/// (8 bytes in the b8 lane); a tagless `DOUBLE_ZERO`/`DOUBLE_ONE` (a 0.0/1.0 with no
/// stored bytes) is not patchable in place and counts as "not found".
///
/// Errors if the block is not v4/v5, or if any path is missing / not a real double
/// / ambiguous.
///
/// A v5 block carrying a binary-blob section (a blobbed `.vmat_c`, `countBlocks >
/// 0`) cannot be shipped uncompressed without the engine misreading its blob
/// framing, so it is **not** rewrapped to `compressionMethod = 0`. Instead it is
/// decompressed to a walkable working copy, patched the same way, then re-emitted
/// still LZ4-compressed (recompressing only the buffer that changed, splicing the
/// blob frames through verbatim). The patch contract is identical either way.
pub fn set_doubles(block: &[u8], edits: &[(Vec<Seg>, f64)]) -> Result<Vec<u8>, DecodeError> {
    // A blobbed LZ4 v5 block is decompressed (but stays logically compressed) so
    // its tail blob framing is preserved on re-emit; everything else rewraps to
    // an uncompressed block that is patched and shipped as-is.
    let blobbed = is_blobbed_lz4_v5(block);
    let mut out = if blobbed {
        decompress_v5_working(block)?
    } else {
        rewrap_uncompressed(block)?
    };
    let version = u32_at(&out, 0)? & 0xFF;
    let min_header = if version == 5 { 120 } else { 72 };
    if out.len() < min_header || (version != 4 && version != 5) {
        return Err(DecodeError::Kv3("double patch requires KV3 v4 or v5"));
    }

    let targets: Vec<&[Seg]> = edits.iter().map(|(p, _)| p.as_slice()).collect();
    let hits = {
        let mut w = PathWalk::new(&out, &targets)?;
        let root = w.read_type()?;
        w.value(root)?;
        w.double_hits
    };

    for i in 0..edits.len() {
        match hits.iter().filter(|h| h.edit == i).count() {
            1 => {}
            0 => {
                return Err(DecodeError::Kv3(
                    "double patch path not found or not a real double",
                ))
            }
            _ => {
                return Err(DecodeError::Kv3(
                    "double patch path is ambiguous (matched more than one field)",
                ))
            }
        }
    }

    for h in &hits {
        let bytes = edits[h.edit].1.to_le_bytes();
        out.get_mut(h.offset..h.offset + 8)
            .ok_or(DecodeError::Kv3("double patch offset out of range"))?
            .copy_from_slice(&bytes);
    }

    if blobbed {
        // Re-emit compressed, recompressing only the buffer the edit landed in and
        // carrying the blob frames through byte-for-byte.
        reassemble_blobbed_v5(block, &out)
    } else {
        Ok(out)
    }
}

/// Sets `FLOAT` (f32) fields located by KV3 path, in place, on a byte-faithful
/// uncompressed re-wrap of `block`. This is the particle-friendly scalar patcher
/// for existing brightness/radius/lifetime-style params that are stored as real
/// f32 values. Tagless numeric constants and doubles are not patched here.
///
/// Errors if the block is not v4/v5, or if any path is missing / not a real float
/// / ambiguous.
pub fn set_floats(block: &[u8], edits: &[(Vec<Seg>, f32)]) -> Result<Vec<u8>, DecodeError> {
    let mut out = rewrap_uncompressed(block)?;
    let version = u32_at(&out, 0)? & 0xFF;
    let min_header = if version == 5 { 120 } else { 72 };
    if out.len() < min_header || (version != 4 && version != 5) {
        return Err(DecodeError::Kv3("float patch requires KV3 v4 or v5"));
    }

    let targets: Vec<&[Seg]> = edits.iter().map(|(p, _)| p.as_slice()).collect();
    let hits = {
        let mut w = PathWalk::new(&out, &targets)?;
        let root = w.read_type()?;
        w.value(root)?;
        w.float_hits
    };

    for i in 0..edits.len() {
        match hits.iter().filter(|h| h.edit == i).count() {
            1 => {}
            0 => {
                return Err(DecodeError::Kv3(
                    "float patch path not found or not a float",
                ))
            }
            _ => {
                return Err(DecodeError::Kv3(
                    "float patch path is ambiguous (matched more than one field)",
                ))
            }
        }
    }

    for h in &hits {
        let bytes = edits[h.edit].1.to_le_bytes();
        out.get_mut(h.offset..h.offset + 4)
            .ok_or(DecodeError::Kv3("float patch offset out of range"))?
            .copy_from_slice(&bytes);
    }
    Ok(out)
}

/// Sets `STRING` fields located by KV3 path by redirecting the field's string id
/// to another string already present in the same KV3 string table.
///
/// This deliberately does **not** add new strings or rewrite the string table:
/// changing table length would be a structural edit. The safe first use is enum
/// probing, e.g. changing an existing `m_nInputMode` to a different
/// `PF_INPUT_MODE_*` value that already appears somewhere in the particle.
/// Passing an empty string writes Source 2's `-1` string id.
///
/// Errors if the block is not v4/v5, if any path is missing / not a string /
/// ambiguous, or if a requested target string is not already interned in the
/// block.
pub fn set_strings(block: &[u8], edits: &[(Vec<Seg>, String)]) -> Result<Vec<u8>, DecodeError> {
    let mut out = rewrap_uncompressed(block)?;
    let version = u32_at(&out, 0)? & 0xFF;
    let min_header = if version == 5 { 120 } else { 72 };
    if out.len() < min_header || (version != 4 && version != 5) {
        return Err(DecodeError::Kv3("string patch requires KV3 v4 or v5"));
    }

    let targets: Vec<&[Seg]> = edits.iter().map(|(p, _)| p.as_slice()).collect();
    let (hits, strings) = {
        let mut w = PathWalk::new(&out, &targets)?;
        let root = w.read_type()?;
        w.value(root)?;
        (w.string_hits, w.strings)
    };

    for i in 0..edits.len() {
        match hits.iter().filter(|h| h.edit == i).count() {
            1 => {}
            0 => {
                return Err(DecodeError::Kv3(
                    "string patch path not found or not a string",
                ))
            }
            _ => {
                return Err(DecodeError::Kv3(
                    "string patch path is ambiguous (matched more than one field)",
                ))
            }
        }
    }

    for h in &hits {
        let target = &edits[h.edit].1;
        let id = if target.is_empty() {
            u32::MAX
        } else {
            u32::try_from(
                strings
                    .iter()
                    .position(|s| s == target)
                    .ok_or(DecodeError::Kv3(
                        "string patch target is not present in the KV3 string table",
                    ))?,
            )
            .map_err(|_| DecodeError::Kv3("string table id does not fit u32"))?
        };
        out.get_mut(h.offset..h.offset + 4)
            .ok_or(DecodeError::Kv3("string patch offset out of range"))?
            .copy_from_slice(&id.to_le_bytes());
    }
    Ok(out)
}

/// Sets boolean fields located by KV3 path, in place, on a byte-faithful
/// uncompressed re-wrap of `block` (preserving structure, as the engine's model
/// loader requires). Each edit is a `(path, value)` resolving to exactly one bool.
///
/// The bool's storage form is preserved: a type-encoded bool (`BOOLEAN_TRUE`/
/// `BOOLEAN_FALSE`, the value is the type byte) has its type byte flipped (keeping
/// the high flag bits); a value-encoded bool (`BOOLEAN` + a 0/1 b1 byte) has that
/// byte set. Built to flip `m_bMeshoptCompressed` in a model's `CTRL` buffer
/// registry when converting a meshopt vertex buffer to an uncompressed one.
///
/// Errors if the block is not v5, or if a path is missing / not a bool / ambiguous.
pub fn set_bools(block: &[u8], edits: &[(Vec<Seg>, bool)]) -> Result<Vec<u8>, DecodeError> {
    let mut out = rewrap_uncompressed(block)?;
    if out.len() < 120 || u32::from_le_bytes([out[0], out[1], out[2], out[3]]) & 0xFF != 5 {
        return Err(DecodeError::Kv3("bool patch requires KV3 v5"));
    }

    let targets: Vec<&[Seg]> = edits.iter().map(|(p, _)| p.as_slice()).collect();
    let hits = {
        let mut w = PathWalk::new(&out, &targets)?;
        let root = w.read_type()?;
        w.value(root)?;
        w.bool_hits
    };

    for i in 0..edits.len() {
        match hits.iter().filter(|h| h.edit == i).count() {
            1 => {}
            0 => return Err(DecodeError::Kv3("bool patch path not found or not a bool")),
            _ => {
                return Err(DecodeError::Kv3(
                    "bool patch path is ambiguous (matched more than one field)",
                ))
            }
        }
    }

    for h in &hits {
        let want = edits[h.edit].1;
        let b = out
            .get_mut(h.offset)
            .ok_or(DecodeError::Kv3("bool patch offset out of range"))?;
        match h.kind {
            // Keep the high flag bits (0x80 = has-flags, 0x40), set the type id.
            BoolKind::TypeByte => {
                *b = (*b & 0xC0)
                    | if want {
                        node::BOOLEAN_TRUE
                    } else {
                        node::BOOLEAN_FALSE
                    };
            }
            BoolKind::ValueByte => *b = u8::from(want),
        }
    }
    Ok(out)
}

/// Encodes `value` to the little-endian bytes of an integer scalar of node type
/// `datatype`, erroring if it does not fit (so the field's storage width is never
/// changed). `INT32_AS_BYTE` reads as an unsigned `u8` (see `reader::read_value`).
#[allow(clippy::wildcard_imports)] // node constants, mirroring reader::read_value
fn fit_scalar(value: i64, datatype: u8) -> Result<Vec<u8>, DecodeError> {
    use node::*;
    let too_big = || DecodeError::Kv3("scalar value does not fit the field's on-disk width");
    Ok(match datatype {
        INT32_AS_BYTE => vec![u8::try_from(value).map_err(|_| too_big())?],
        INT16 => i16::try_from(value)
            .map_err(|_| too_big())?
            .to_le_bytes()
            .to_vec(),
        UINT16 => u16::try_from(value)
            .map_err(|_| too_big())?
            .to_le_bytes()
            .to_vec(),
        INT32 => i32::try_from(value)
            .map_err(|_| too_big())?
            .to_le_bytes()
            .to_vec(),
        UINT32 => u32::try_from(value)
            .map_err(|_| too_big())?
            .to_le_bytes()
            .to_vec(),
        INT64 => value.to_le_bytes().to_vec(),
        UINT64 => u64::try_from(value)
            .map_err(|_| too_big())?
            .to_le_bytes()
            .to_vec(),
        _ => {
            return Err(DecodeError::Kv3(
                "target field is not a settable integer scalar",
            ))
        }
    })
}

/// A located scalar field: which edit it satisfies, its absolute byte offset, and
/// its node type (so the value is fitted to the right width).
struct Hit {
    edit: usize,
    offset: usize,
    datatype: u8,
}

/// A located boolean field, for [`set_bools`]. A KV3 bool is stored either as its
/// own type byte (`BOOLEAN_TRUE` / `BOOLEAN_FALSE`, the value *is* the type) or as
/// a `BOOLEAN` type with a 0/1 value byte in the b1 lane; `kind` says which byte
/// to patch.
struct BoolHit {
    edit: usize,
    offset: usize,
    kind: BoolKind,
}

#[derive(Clone, Copy)]
enum BoolKind {
    /// The byte at `offset` is the type byte: set its low 6 bits to
    /// `BOOLEAN_TRUE`/`BOOLEAN_FALSE`, preserving the high flag bits.
    TypeByte,
    /// The byte at `offset` is a b1 value byte: set it to 0/1.
    ValueByte,
}

/// Path-tracking sibling of [`Walk`]: walks the value tree (sharing [`lanes`]),
/// maintaining the current KV3 path, and records each integer scalar whose path
/// equals one of `targets`. Used by [`set_scalars`].
struct PathWalk<'a> {
    block: &'a [u8],
    /// KV3 version (4 or 5): selects the OBJECT member-count source (b4 lane for
    /// v4, the object-length lane for v5).
    version: u32,
    types: Lane,
    obj_lengths: Lane,
    main: [Lane; 4],
    aux: [Lane; 4],
    strings: Vec<String>,
    targets: &'a [&'a [Seg]],
    path: Vec<Seg>,
    hits: Vec<Hit>,
    bool_hits: Vec<BoolHit>,
    double_hits: Vec<Hit>,
    float_hits: Vec<Hit>,
    string_hits: Vec<Hit>,
}

impl<'a> PathWalk<'a> {
    fn new(block: &'a [u8], targets: &'a [&'a [Seg]]) -> Result<Self, DecodeError> {
        let version = u32_at(block, 0)? & 0xFF;
        let l = lanes(block, version)?;
        Ok(PathWalk {
            block,
            version,
            types: l.types,
            obj_lengths: l.obj_lengths,
            main: l.main,
            aux: l.aux,
            strings: l.strings,
            targets,
            path: Vec::new(),
            hits: Vec::new(),
            bool_hits: Vec::new(),
            double_hits: Vec::new(),
            float_hits: Vec::new(),
            string_hits: Vec::new(),
        })
    }

    fn read_type(&mut self) -> Result<u8, DecodeError> {
        let mut t = *self
            .block
            .get(self.types.at())
            .ok_or(DecodeError::Kv3("type stream underrun"))?;
        self.types.pos += 1;
        if t & 0x80 != 0 {
            t &= 0x3F;
            self.types.pos += 1;
        }
        Ok(t)
    }

    fn lane_u32(&mut self, lane: usize) -> Result<u32, DecodeError> {
        let v = u32_at(self.block, self.main[lane].at())?;
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

    /// Records a type-encoded bool (`BOOLEAN_TRUE`/`BOOLEAN_FALSE`) as a hit if its
    /// path matches a target. `type_off` is the absolute offset of its type byte.
    fn record_bool_type(&mut self, datatype: u8, type_off: usize) {
        if datatype != node::BOOLEAN_TRUE && datatype != node::BOOLEAN_FALSE {
            return;
        }
        if let Some(edit) = self.targets.iter().position(|t| self.path.as_slice() == *t) {
            self.bool_hits.push(BoolHit {
                edit,
                offset: type_off,
                kind: BoolKind::TypeByte,
            });
        }
    }

    /// Records a value-encoded bool (`BOOLEAN` + a 0/1 b1 byte) as a hit if its
    /// path matches a target. `value_off` is the absolute offset of the b1 byte.
    fn record_bool_value(&mut self, value_off: usize) {
        if let Some(edit) = self.targets.iter().position(|t| self.path.as_slice() == *t) {
            self.bool_hits.push(BoolHit {
                edit,
                offset: value_off,
                kind: BoolKind::ValueByte,
            });
        }
    }

    /// Records the current real `DOUBLE` (b8, 8 bytes) as a hit if its path
    /// matches a target. Tagless `DOUBLE_ZERO`/`DOUBLE_ONE` carry no bytes and so
    /// are never recorded (they cannot be patched in place).
    fn record_double(&mut self) {
        let offset = self.main[B8].at();
        if let Some(edit) = self.targets.iter().position(|t| self.path.as_slice() == *t) {
            self.double_hits.push(Hit {
                edit,
                offset,
                datatype: node::DOUBLE,
            });
        }
    }

    /// Records the current real `FLOAT` (b4, 4 bytes) as a hit if its path
    /// matches a target.
    fn record_float(&mut self) {
        let offset = self.main[B4].at();
        if let Some(edit) = self.targets.iter().position(|t| self.path.as_slice() == *t) {
            self.float_hits.push(Hit {
                edit,
                offset,
                datatype: node::FLOAT,
            });
        }
    }

    /// Records the current `STRING` id (b4, 4 bytes) as a hit if its path matches
    /// a target.
    fn record_string(&mut self) {
        let offset = self.main[B4].at();
        if let Some(edit) = self.targets.iter().position(|t| self.path.as_slice() == *t) {
            self.string_hits.push(Hit {
                edit,
                offset,
                datatype: node::STRING,
            });
        }
    }

    /// Records the current scalar as a hit if its path matches a target.
    fn record(&mut self, lane: usize, datatype: u8) {
        let offset = self.main[lane].at();
        let matched = self.targets.iter().position(|t| self.path.as_slice() == *t);
        if let Some(edit) = matched {
            self.hits.push(Hit {
                edit,
                offset,
                datatype,
            });
        }
    }

    /// Walks one value, advancing every cursor exactly as the reader does and
    /// pushing/popping `path` on each descent, recording matching scalars.
    #[allow(clippy::wildcard_imports)] // node constants, mirroring reader::read_value
    fn value(&mut self, datatype: u8) -> Result<(), DecodeError> {
        use node::*;
        match datatype {
            INT32 | UINT32 => {
                self.record(B4, datatype);
                self.main[B4].pos += 4;
            }
            FLOAT => {
                self.record_float();
                self.main[B4].pos += 4;
            }
            STRING => {
                self.record_string();
                self.main[B4].pos += 4;
            }
            INT64 | UINT64 => {
                self.record(B8, datatype);
                self.main[B8].pos += 8;
            }
            DOUBLE => {
                self.record_double();
                self.main[B8].pos += 8;
            }
            INT16 | UINT16 => {
                self.record(B2, datatype);
                self.main[B2].pos += 2;
            }
            INT32_AS_BYTE => {
                self.record(B1, datatype);
                self.main[B1].pos += 1;
            }
            BOOLEAN => {
                self.record_bool_value(self.main[B1].at());
                self.main[B1].pos += 1;
            }
            // BINARY_BLOB consumes nothing from the typed lanes (its bytes live in
            // the separate blob region the reader pulls from), so it advances no
            // cursor here, like the tagless constants.
            NULL | BOOLEAN_TRUE | BOOLEAN_FALSE | INT64_ZERO | INT64_ONE | DOUBLE_ZERO
            | DOUBLE_ONE | BINARY_BLOB => {}
            ARRAY => {
                let n = self.lane_u32(B4)?;
                for i in 0..n {
                    let type_off = self.types.at();
                    let t = self.read_type()?;
                    self.path.push(Seg::Index(i as usize));
                    self.record_bool_type(t, type_off);
                    self.value(t)?;
                    self.path.pop();
                }
            }
            ARRAY_TYPED => {
                let n = self.lane_u32(B4)?;
                let sub = self.read_type()?;
                for i in 0..n {
                    self.path.push(Seg::Index(i as usize));
                    self.value(sub)?;
                    self.path.pop();
                }
            }
            ARRAY_TYPE_BYTE_LENGTH => {
                let n = u32::from(self.lane_u8(B1)?);
                let sub = self.read_type()?;
                for i in 0..n {
                    self.path.push(Seg::Index(i as usize));
                    self.value(sub)?;
                    self.path.pop();
                }
            }
            ARRAY_TYPE_AUXILIARY_BUFFER => {
                let n = u32::from(self.lane_u8(B1)?);
                let sub = self.read_type()?;
                std::mem::swap(&mut self.main, &mut self.aux);
                for i in 0..n {
                    self.path.push(Seg::Index(i as usize));
                    self.value(sub)?;
                    self.path.pop();
                }
                std::mem::swap(&mut self.main, &mut self.aux);
            }
            OBJECT => {
                // v5 reads the member count from the object-length lane; v4 reads
                // it from the b4 lane inline (it has no object-length lane).
                let n = if self.version >= 5 {
                    self.obj_len()?
                } else {
                    self.lane_u32(B4)?
                };
                for _ in 0..n {
                    let type_off = self.types.at();
                    let vt = self.read_type()?;
                    let id = self.lane_u32(B4)?;
                    self.path.push(Seg::Key(self.key(id).to_string()));
                    self.record_bool_type(vt, type_off);
                    self.value(vt)?;
                    self.path.pop();
                }
            }
            other => return Err(DecodeError::Kv3NodeType(other)),
        }
        Ok(())
    }
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

/// The lane/cursor layout of a rewrapped, uncompressed v5 KV3 block: where each
/// typed lane (b1/b2/b4/b8) of the aux and main buffers begins, the type stream
/// and object-length cursors, and the decoded strings. Shared by every walker so
/// the (fragile) layout math is computed in exactly one place.
struct Lanes {
    types: Lane,
    obj_lengths: Lane,
    main: [Lane; 4],
    aux: [Lane; 4],
    strings: Vec<String>,
}

/// Computes the [`Lanes`] of an uncompressed block, dispatching on KV3 version:
/// v5's two-buffer layout or v4's single buffer. (v1..=3 are not patched in place.)
fn lanes(block: &[u8], version: u32) -> Result<Lanes, DecodeError> {
    match version {
        5 => lanes_v5(block),
        4 => lanes_v4(block),
        _ => Err(DecodeError::Kv3("KV3 in-place patch supports only v4/v5")),
    }
}

/// Computes the [`Lanes`] of an uncompressed **v4** block. Mirrors `reader`'s
/// `layout_single`: one buffer at offset 72 laid out as
/// `[b1][align2 b2][align4 b4][align8 b8][strings][types][trailer]`. The string
/// count is the first int of b4 (so the b4 value lane starts 4 bytes in); strings
/// and types are inline regions, not sub-buffers. There is no object-length lane
/// (OBJECT member counts come from b4 at walk time) and no auxiliary buffer.
fn lanes_v4(block: &[u8]) -> Result<Lanes, DecodeError> {
    const BUF: usize = 72; // single buffer base in an uncompressed v4 block

    // v4 header counts (see reader::decode).
    let count_b1 = i32_at(block, 28)? as usize;
    let count_b4 = i32_at(block, 32)? as usize;
    let count_b8 = i32_at(block, 36)? as usize;
    let count_b2 = i32_at(block, 64)? as usize;

    let mut off = 0usize;
    let b1_start = off;
    off += count_b1;
    let (b2_start, _) = lane(&mut off, count_b2, 2);
    let (b4_start, _) = lane(&mut off, count_b4, 4);
    // b8 is 8-aligned whether or not it is present (mirrors `layout_single`); the
    // string region begins immediately after it.
    let b8_start = if count_b8 > 0 {
        lane(&mut off, count_b8, 8).0
    } else {
        align(&mut off, 8);
        off
    };

    // String count is the first int of b4; the strings themselves are the
    // null-terminated run at `off`, and the type stream follows them.
    let string_count = u32::try_from(i32_at(block, BUF + b4_start)?)
        .map_err(|_| DecodeError::Kv3("negative string count"))? as usize;
    let mut sp = BUF + off;
    let mut strings = Vec::with_capacity(string_count);
    for _ in 0..string_count {
        strings.push(read_cstr(block, &mut sp)?);
    }
    let types = Lane { base: sp, pos: 0 };

    let main = [
        Lane {
            base: BUF + b1_start,
            pos: 0,
        },
        Lane {
            base: BUF + b2_start,
            pos: 0,
        },
        Lane {
            base: BUF + b4_start + 4, // skip the leading string count
            pos: 0,
        },
        Lane {
            base: BUF + b8_start,
            pos: 0,
        },
    ];
    // v4 has no object-length lane and no auxiliary buffer; those lanes are never
    // read while walking a v4 block, so an empty placeholder is correct.
    let empty = Lane { base: 0, pos: 0 };
    Ok(Lanes {
        types,
        obj_lengths: empty,
        main,
        aux: [empty, empty, empty, empty],
        strings,
    })
}

/// Computes the [`Lanes`] of an uncompressed v5 block. Mirrors `reader::decode`'s
/// field offsets and buffer layout.
fn lanes_v5(block: &[u8]) -> Result<Lanes, DecodeError> {
    // Header counts (v5). Aux counts are the "first" count block; main counts
    // sit in the v5-specific tail.
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
        .map_err(|_| DecodeError::Kv3("negative string count"))? as usize;
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

    Ok(Lanes {
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
    })
}

impl<'a> Walk<'a> {
    fn new(block: &'a [u8], targets: &'a [(usize, usize)]) -> Result<Self, DecodeError> {
        // `neutralize_draw_calls` (the only Walk caller) is v5-only and has already
        // verified the block is v5 before constructing the walker.
        let l = lanes(block, 5)?;
        Ok(Walk {
            block,
            types: l.types,
            obj_lengths: l.obj_lengths,
            main: l.main,
            aux: l.aux,
            strings: l.strings,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv3::Value;
    use crate::resource::Resource;

    /// A real Deadlock material (Graves' wall-of-hands energy). Its green ability
    /// color is a `g_vColorTint` / `g_vSelfIllumTint` constant, and its `DATA` block
    /// is the hard shape: KV3 v5, LZ4-compressed, carrying a binary-blob section
    /// (`countBlocks = 1`). `rewrap_uncompressed` refuses this (a `comp = 0` re-emit
    /// misframes the blob and the engine renders the covered mesh as a red error
    /// material), so `set_doubles` must patch it and re-emit STILL compressed.
    const NECRO_HANDS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/material/necro_hands.vmat_c"
    ));

    fn field(b: &[u8], o: usize) -> i32 {
        i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    /// buf2 (main, compressed) and the binary-blob frame region of a v5 DATA block,
    /// located the way the engine does: sequentially, from the buffer size fields.
    fn buf2_and_frames(d: &[u8]) -> (&[u8], &[u8]) {
        let comp1 = usize::try_from(field(d, 76)).unwrap();
        let comp2 = usize::try_from(field(d, 84)).unwrap();
        let b2 = 120 + comp1;
        let frames = b2 + comp2;
        (&d[b2..frames], &d[frames..])
    }

    #[test]
    fn set_doubles_patches_a_blobbed_v5_material_keeping_it_compressed() {
        let res = Resource::parse(NECRO_HANDS).expect("parse resource");
        let data = res.data_block().expect("DATA block");

        // Precondition: the committed fixture really is the hard case.
        assert_eq!(u32_at(data, 0).unwrap() & 0xFF, 5, "v5");
        assert_eq!(u32_at(data, 20).unwrap(), 1, "LZ4-compressed");
        assert_eq!(field(data, 56), 1, "one binary blob");

        // Locate a real-double tint channel (channel 0 of the first tint param; the
        // tagless 1.0 alpha is not patchable, but RGB are stored f64s).
        let tree = crate::kv3::decode(data).expect("decode tree");
        let params = tree
            .get("m_vectorParams")
            .and_then(Value::as_array)
            .expect("m_vectorParams");
        let (pi, param) = params
            .iter()
            .enumerate()
            .find(|(_, p)| {
                p.get("m_name").and_then(Value::as_str).is_some_and(|n| {
                    n.starts_with("g_vColorTint") || n.starts_with("g_vSelfIllumTint")
                })
            })
            .expect("a tint param");
        let chan0 = param
            .get("m_value")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_f64)
            .expect("channel 0");
        // A real DOUBLE node, not the tagless 0.0/1.0 (compare bits to stay clear of
        // clippy::float_cmp; the point is that it has stored bytes to patch).
        assert!(
            chan0.to_bits() != 0.0f64.to_bits() && chan0.to_bits() != 1.0f64.to_bits(),
            "channel 0 must be a stored double, got {chan0}"
        );

        let path = vec![
            Seg::Key("m_vectorParams".to_string()),
            Seg::Index(pi),
            Seg::Key("m_value".to_string()),
            Seg::Index(0),
        ];
        let new0 = 0.123_456_f64;
        let patched = set_doubles(data, &[(path, new0)]).expect("patch blobbed double");

        // 1. Still the engine-loadable compressed + blobbed shape: it was NOT flipped
        //    to the broken comp=0 form, and the blob framing fields are untouched.
        assert_eq!(u32_at(&patched, 0).unwrap() & 0xFF, 5);
        assert_eq!(u32_at(&patched, 20).unwrap(), 1, "stays LZ4-compressed");
        assert_eq!(field(&patched, 56), 1, "blob section preserved");
        assert_eq!(field(&patched, 60), field(data, 60), "sizeBlobs unchanged");
        assert_eq!(
            field(&patched, 68),
            field(data, 68),
            "sizeBlockCompressed unchanged"
        );
        assert_eq!(
            field(&patched, 48),
            field(data, 48),
            "sizeUncTotal unchanged"
        );
        assert_eq!(field(&patched, 80), field(data, 80), "unc2 unchanged");
        // size_comp_total (52) stays consistent with comp1 + comp2.
        assert_eq!(
            field(&patched, 52),
            field(&patched, 76) + field(&patched, 84),
            "size_comp_total == comp1 + comp2"
        );

        // 2. Only the patched channel changed: rebuild the expected tree by editing
        //    that one channel and require FULL tree equality, which proves every other
        //    field, including the binary blob (a Value::Binary node), is unchanged.
        let new_tree = crate::kv3::decode(&patched).expect("decode patched");
        let mut expect = tree.clone();
        if let Some(Value::Array(ps)) = expect.get_mut("m_vectorParams") {
            if let Some(Value::Array(ch)) = ps[pi].get_mut("m_value") {
                ch[0] = Value::Double(new0);
            }
        }
        assert_eq!(new_tree, expect, "only the targeted tint channel changed");

        // 3. Raw faithfulness (the Approach-A guarantee): the tint doubles live in
        //    buf1 (the aux buffer), so buf2 and the blob frames are byte-identical;
        //    only buf1 was recompressed.
        let (buf2_old, frames_old) = buf2_and_frames(data);
        let (buf2_new, frames_new) = buf2_and_frames(&patched);
        assert_eq!(buf2_new, buf2_old, "buf2 (main) byte-identical");
        assert_eq!(frames_new, frames_old, "blob frames byte-identical");
    }
}
