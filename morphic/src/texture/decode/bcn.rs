//! `BCn` block decompression via `bcdec_rs` (safe pure-Rust port of bcdec.h).
//!
//! The caller (`texture::pixel_data`) has already sliced `pixels` to exactly
//! mip 0's bytes, so this module deals with a single mip and doesn't need to
//! know about the mip chain.

use crate::error::DecodeError;
use crate::texture::{Image, ImageData, TextureInfo};

const BC1_BLOCK_BYTES: usize = 8;
const BC3_BLOCK_BYTES: usize = 16;
const BC4_BLOCK_BYTES: usize = 8;
const BC5_BLOCK_BYTES: usize = 16;
const BC7_BLOCK_BYTES: usize = 16;

/// BC1 (DXT1): RGBA output, 8 bytes/block.
pub fn decode_bc1(info: &TextureInfo, pixels: &[u8]) -> Result<Image, DecodeError> {
    decode_to_rgba(info, pixels, BC1_BLOCK_BYTES, bcdec_rs::bc1)
}

/// BC3 (DXT5): RGBA output with alpha block, 16 bytes/block.
pub fn decode_bc3(info: &TextureInfo, pixels: &[u8]) -> Result<Image, DecodeError> {
    decode_to_rgba(info, pixels, BC3_BLOCK_BYTES, bcdec_rs::bc3)
}

/// BC7: RGBA output, 16 bytes/block, 8 mode variants. By count the dominant
/// `BCn` format in Deadlock.
pub fn decode_bc7(info: &TextureInfo, pixels: &[u8]) -> Result<Image, DecodeError> {
    decode_to_rgba(info, pixels, BC7_BLOCK_BYTES, bcdec_rs::bc7)
}

/// BC4 (ATI1N): single-channel R, 8 bytes/block. Replicated to RGB with
/// alpha = 255 to match VRF's default output for ungated textures.
pub fn decode_bc4(info: &TextureInfo, pixels: &[u8]) -> Result<Image, DecodeError> {
    let (w, h) = check(info, pixels, BC4_BLOCK_BYTES)?;
    let (wu, hu) = (usize::from(w), usize::from(h));
    let mut r = vec![0u8; wu * hu];
    iter_blocks_into(
        wu,
        hu,
        pixels,
        BC4_BLOCK_BYTES,
        &mut r,
        1,
        |block, dst, pitch| {
            bcdec_rs::bc4(block, dst, pitch, false);
        },
    );
    Ok(splat_r_to_rgba(w, h, &r))
}

/// BC5 (ATI2N): R + G, 16 bytes/block. Output as RGBA with B = 0, A = 255
/// (raw VRF emission when no channel remap is requested).
pub fn decode_bc5(info: &TextureInfo, pixels: &[u8]) -> Result<Image, DecodeError> {
    let (w, h) = check(info, pixels, BC5_BLOCK_BYTES)?;
    let (wu, hu) = (usize::from(w), usize::from(h));
    let mut rg = vec![0u8; wu * hu * 2];
    iter_blocks_into(
        wu,
        hu,
        pixels,
        BC5_BLOCK_BYTES,
        &mut rg,
        2,
        |block, dst, pitch| {
            bcdec_rs::bc5(block, dst, pitch, false);
        },
    );
    Ok(rg_to_rgba(w, h, &rg))
}

// --- internals ---

fn decode_to_rgba(
    info: &TextureInfo,
    pixels: &[u8],
    block_bytes: usize,
    decode_fn: fn(&[u8], &mut [u8], usize),
) -> Result<Image, DecodeError> {
    let (w, h) = check(info, pixels, block_bytes)?;
    let (wu, hu) = (usize::from(w), usize::from(h));
    let mut rgba = vec![0u8; wu * hu * 4];
    iter_blocks_into(
        wu,
        hu,
        pixels,
        block_bytes,
        &mut rgba,
        4,
        |block, dst, pitch| {
            decode_fn(block, dst, pitch);
        },
    );
    Ok(Image {
        width: u32::from(w),
        height: u32::from(h),
        data: ImageData::Rgba8(rgba),
    })
}

fn check(info: &TextureInfo, pixels: &[u8], block_bytes: usize) -> Result<(u16, u16), DecodeError> {
    let blocks_x = usize::from(info.width).div_ceil(4);
    let blocks_y = usize::from(info.height).div_ceil(4);
    let needed = blocks_x * blocks_y * block_bytes;
    if pixels.len() < needed {
        return Err(DecodeError::Truncated {
            offset: 0,
            needed,
            had: pixels.len(),
        });
    }
    Ok((info.width, info.height))
}

fn iter_blocks_into(
    width: usize,
    height: usize,
    pixels: &[u8],
    block_bytes: usize,
    dst: &mut [u8],
    bytes_per_pixel: usize,
    mut decode_block: impl FnMut(&[u8], &mut [u8], usize),
) {
    let pitch = width * bytes_per_pixel;
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block_idx = by * blocks_x + bx;
            let block = &pixels[block_idx * block_bytes..(block_idx + 1) * block_bytes];
            let dst_offset = (by * 4) * pitch + (bx * 4) * bytes_per_pixel;
            decode_block(block, &mut dst[dst_offset..], pitch);
        }
    }
}

fn splat_r_to_rgba(width: u16, height: u16, red: &[u8]) -> Image {
    let n = usize::from(width) * usize::from(height);
    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        let v = red[i];
        rgba[i * 4] = v;
        rgba[i * 4 + 1] = v;
        rgba[i * 4 + 2] = v;
        rgba[i * 4 + 3] = 255;
    }
    Image {
        width: u32::from(width),
        height: u32::from(height),
        data: ImageData::Rgba8(rgba),
    }
}

fn rg_to_rgba(w: u16, h: u16, rg: &[u8]) -> Image {
    let n = usize::from(w) * usize::from(h);
    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        rgba[i * 4] = rg[i * 2];
        rgba[i * 4 + 1] = rg[i * 2 + 1];
        rgba[i * 4 + 2] = 0;
        rgba[i * 4 + 3] = 255;
    }
    Image {
        width: u32::from(w),
        height: u32::from(h),
        data: ImageData::Rgba8(rgba),
    }
}
