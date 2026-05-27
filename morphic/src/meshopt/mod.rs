//! meshoptimizer codec decoders for Source 2 `MVTX` (vertex) and `MIDX`
//! (index) mesh buffers. Pure-Rust ports of `ValveResourceFormat.Compression`
//! (scalar paths), which themselves port zeux's meshoptimizer. Kept pure-Rust
//! to preserve morphic's no-C-toolchain build; validated byte-exact against the
//! VRF oracle.

// First consumer is the model decoder (M3+); reachable only from tests until
// then, so the public re-exports read as dead/unused for now.
#![allow(dead_code, unused_imports)]

mod index;
mod vertex;

pub use index::decode_index_buffer;
pub use vertex::decode_vertex_buffer;

#[cfg(test)]
mod tests;
