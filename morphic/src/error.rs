use crate::texture::format::TextureFormat;

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("not yet implemented: {0:?}")]
    Unimplemented(TextureFormat),

    #[error("input too short at offset {offset}: needed {needed} bytes, had {had}")]
    Truncated {
        offset: u64,
        needed: usize,
        had: usize,
    },

    #[error("malformed resource header: {0}")]
    BadResource(&'static str),

    #[error("resource has no DATA block")]
    MissingDataBlock,

    #[error("unsupported KV3 version: 0x{0:08x}")]
    UnsupportedKv3(u32),

    #[error("KV3 parse error: {0}")]
    Kv3(&'static str),

    #[error("texture header missing field: {0}")]
    MissingField(&'static str),

    #[error("unknown texture format id: {0}")]
    UnknownFormat(i32),

    #[error("unknown texture format name")]
    UnknownFormatName,

    #[error("invalid decode target: mip {mip}, slice {slice}, face {face}")]
    InvalidTarget { mip: u8, slice: u16, face: u8 },

    #[error("inline image decode failed: {0}")]
    InlineImage(String),
}
