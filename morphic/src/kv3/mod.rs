//! Binary KV3 parser. The 4-byte magic selects the version; per-version
//! decoding lives in [`binary`].
//!
//! Promoted to `pub` when a second caller (e.g. Grimoire) needs raw KV3
//! access; currently `pub(crate)` to keep the public surface minimal.

// KV3 isn't used by texture parsing (textures have a fixed binary header).
// The model decoder (M2+) is the first consumer; until then `parse` and the
// `Value` accessors are reachable only from tests.
#![allow(dead_code)]

mod binary;
mod types;

pub use types::Value;

use crate::error::DecodeError;

/// Parses a self-contained binary KV3 block (`DATA`, `MDAT`, a `.vmat_c` data
/// block, ...) into a [`Value`] tree. Deadlock hero models pin KV3 v5.
pub fn parse(data: &[u8]) -> Result<Value, DecodeError> {
    binary::parse(data)
}

#[cfg(test)]
mod tests;
