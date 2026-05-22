//! Block-compressed encoders backed by `intel_tex_2` (Intel's ISPC texture
//! compressor). Inverse of [`crate::texture::decode::bcn`].
//!
//! All `BCn` formats operate on 4x4 pixel blocks, so width and height must be
//! multiples of 4. Source 2 stores `actual_width` / `actual_height` in the
//! KV3 metadata for `NonPow2` textures; the on-wire dims (`info.width`,
//! `info.height`) are already padded to a block multiple by VRF. The
//! fixtures we test against all satisfy this, so for Phase 2 we reject
//! non-block-aligned inputs rather than pad. Pad-on-encode can come later
//! if a real workflow needs it.
//!
//! Channel layout:
//! - BC1 / BC3 / BC7: full RGBA input
//! - BC4 (Ati1n): extract red channel only
//! - BC5 (Ati2n): extract red + green channels
//! - BC6H: HDR; expects RGBA f16 input, four channels packed as 8 bytes per pixel

use intel_tex_2::{bc1, bc3, bc4, bc5, bc6h, bc7, RSurface, RgSurface, RgbaSurface};

use crate::error::EncodeError;
use crate::texture::format::TextureFormat;
use crate::texture::{Image, ImageData};

pub fn encode_bc1(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let buf = require_rgba8(image, TextureFormat::Dxt1)?;
    check_block_aligned(image, TextureFormat::Dxt1)?;
    check_rgba8_len(buf, image, TextureFormat::Dxt1)?;
    let surface = RgbaSurface {
        width: image.width,
        height: image.height,
        stride: image.width * 4,
        data: buf,
    };
    let mut out = vec![0u8; bc1::calc_output_size(image.width, image.height)];
    bc1::compress_blocks_into(&surface, &mut out);
    Ok(out)
}

pub fn encode_bc3(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let buf = require_rgba8(image, TextureFormat::Dxt5)?;
    check_block_aligned(image, TextureFormat::Dxt5)?;
    check_rgba8_len(buf, image, TextureFormat::Dxt5)?;
    let surface = RgbaSurface {
        width: image.width,
        height: image.height,
        stride: image.width * 4,
        data: buf,
    };
    let mut out = vec![0u8; bc3::calc_output_size(image.width, image.height)];
    bc3::compress_blocks_into(&surface, &mut out);
    Ok(out)
}

pub fn encode_bc4(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let buf = require_rgba8(image, TextureFormat::Ati1n)?;
    check_block_aligned(image, TextureFormat::Ati1n)?;
    check_rgba8_len(buf, image, TextureFormat::Ati1n)?;
    // BC4 takes one channel. Extract red.
    let pixel_count = (image.width as usize) * (image.height as usize);
    let mut r = Vec::with_capacity(pixel_count);
    for px in buf.chunks_exact(4) {
        r.push(px[0]);
    }
    let surface = RSurface {
        width: image.width,
        height: image.height,
        stride: image.width,
        data: &r,
    };
    let mut out = vec![0u8; bc4::calc_output_size(image.width, image.height)];
    bc4::compress_blocks_into(&surface, &mut out);
    Ok(out)
}

pub fn encode_bc5(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let buf = require_rgba8(image, TextureFormat::Ati2n)?;
    check_block_aligned(image, TextureFormat::Ati2n)?;
    check_rgba8_len(buf, image, TextureFormat::Ati2n)?;
    // BC5 takes two channels. Extract red + green into interleaved RG.
    let pixel_count = (image.width as usize) * (image.height as usize);
    let mut rg = Vec::with_capacity(pixel_count * 2);
    for px in buf.chunks_exact(4) {
        rg.push(px[0]);
        rg.push(px[1]);
    }
    let surface = RgSurface {
        width: image.width,
        height: image.height,
        stride: image.width * 2,
        data: &rg,
    };
    let mut out = vec![0u8; bc5::calc_output_size(image.width, image.height)];
    bc5::compress_blocks_into(&surface, &mut out);
    Ok(out)
}

pub fn encode_bc7(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let buf = require_rgba8(image, TextureFormat::Bc7)?;
    check_block_aligned(image, TextureFormat::Bc7)?;
    check_rgba8_len(buf, image, TextureFormat::Bc7)?;
    let surface = RgbaSurface {
        width: image.width,
        height: image.height,
        stride: image.width * 4,
        data: buf,
    };
    // alpha_basic_settings is a reasonable default: handles alpha correctly
    // for textures that have it and isn't catastrophically slow. Quality
    // knobs can be exposed later if needed.
    let settings = bc7::alpha_basic_settings();
    let mut out = vec![0u8; bc7::calc_output_size(image.width, image.height)];
    bc7::compress_blocks_into(&settings, &surface, &mut out);
    Ok(out)
}

pub fn encode_bc6h(image: &Image) -> Result<Vec<u8>, EncodeError> {
    let ImageData::Rgba16F(pixels) = &image.data else {
        return Err(EncodeError::WrongPixelKind {
            format: TextureFormat::Bc6h,
            reason: "expected Rgba16F pixels, got Rgba8",
        });
    };
    check_block_aligned(image, TextureFormat::Bc6h)?;
    let expected_pixels = (image.width as usize) * (image.height as usize) * 4;
    if pixels.len() != expected_pixels {
        return Err(EncodeError::SizeMismatch {
            format: TextureFormat::Bc6h,
            width: image.width,
            height: image.height,
            expected: expected_pixels * 2,
            got: pixels.len() * 2,
        });
    }
    let bytes: &[u8] = bytemuck::cast_slice(pixels);
    let surface = RgbaSurface {
        width: image.width,
        height: image.height,
        stride: image.width * 8, // 4 channels * f16 (2 bytes)
        data: bytes,
    };
    // M7's decoder uses bcdec_rs::bc6h_half (unsigned). intel_tex_2's BC6H
    // encoder is the matching unsigned UF16 variant, so decode->encode round
    // trips with the same interpretation.
    let settings = bc6h::basic_settings();
    let mut out = vec![0u8; bc6h::calc_output_size(image.width, image.height)];
    bc6h::compress_blocks_into(&settings, &surface, &mut out);
    Ok(out)
}

fn require_rgba8(image: &Image, format: TextureFormat) -> Result<&[u8], EncodeError> {
    match &image.data {
        ImageData::Rgba8(buf) => Ok(buf),
        ImageData::Rgba16F(_) => Err(EncodeError::WrongPixelKind {
            format,
            reason: "expected Rgba8 pixels, got Rgba16F",
        }),
    }
}

fn check_rgba8_len(buf: &[u8], image: &Image, format: TextureFormat) -> Result<(), EncodeError> {
    let expected = (image.width as usize) * (image.height as usize) * 4;
    if buf.len() != expected {
        return Err(EncodeError::SizeMismatch {
            format,
            width: image.width,
            height: image.height,
            expected,
            got: buf.len(),
        });
    }
    Ok(())
}

fn check_block_aligned(image: &Image, format: TextureFormat) -> Result<(), EncodeError> {
    if image.width.is_multiple_of(4) && image.height.is_multiple_of(4) {
        Ok(())
    } else {
        Err(EncodeError::WrongPixelKind {
            format,
            reason: "BCn dims must be a multiple of 4 (Phase 2 does not pad)",
        })
    }
}
