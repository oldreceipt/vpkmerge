//! Source 2 resource container: header + block table.
//!
//! See [`header`] for the binary layout. We only need DATA for texture
//! inspect; RERL/REDI/NTRO are skipped over by ignoring those block entries.

mod header;

pub use header::{Block, Resource};

use crate::error::DecodeError;

pub const BLOCK_TYPE_DATA: [u8; 4] = *b"DATA";

impl<'a> Resource<'a> {
    pub fn data_block(&self) -> Result<&'a [u8], DecodeError> {
        self.find_block(BLOCK_TYPE_DATA)
            .ok_or(DecodeError::MissingDataBlock)
    }

    pub fn data_block_meta(&self) -> Result<Block, DecodeError> {
        self.find_block_meta(BLOCK_TYPE_DATA)
            .ok_or(DecodeError::MissingDataBlock)
    }
}
