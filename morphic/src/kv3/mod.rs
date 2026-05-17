//! Binary KV3 parser. Versions are dispatched at parse time by reading the
//! first 4 magic bytes; per-version code lives in sibling modules.
//!
//! Promoted to `pub` when a second caller (e.g. Grimoire) needs raw KV3
//! access; currently `pub(crate)` to keep the public surface minimal.

// KV3 isn't used by texture parsing (textures have a fixed binary header).
// Kept as a future home for material/model resource parsing.
#![allow(dead_code)]

mod types;

pub use types::Value;

use crate::error::DecodeError;

pub fn parse(_data: &[u8]) -> Result<Value, DecodeError> {
    Err(DecodeError::Kv3("kv3::parse not yet implemented"))
}
