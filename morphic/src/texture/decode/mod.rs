//! Pixel decode dispatch. Each format gets its own submodule; the dispatcher
//! lives here and is intentionally exhaustive so adding a format requires
//! either implementing it or extending the match.

mod bcn;
mod inline;
mod rgba8;

use crate::error::DecodeError;
use crate::texture::{Image, TextureInfo};
use crate::DecodeOptions;

use super::format::TextureFormat;

pub fn decode_image(
    info: &TextureInfo,
    pixels: &[u8],
    opts: &DecodeOptions,
) -> Result<Image, DecodeError> {
    // face is validated and sliced in pixel_data; mip and slice are still
    // M9-only here. Lower-mip selection (M9 follow-up) and 3D/array slices
    // (M10 remainder) are pending.
    if opts.mip != 0 || opts.slice != 0 {
        return Err(DecodeError::InvalidTarget {
            mip: opts.mip,
            slice: opts.slice,
            face: opts.face,
        });
    }
    match info.format {
        TextureFormat::Rgba8888 => rgba8::decode_rgba(info, pixels),
        TextureFormat::Bgra8888 => rgba8::decode_bgra(info, pixels),
        TextureFormat::Dxt1 => bcn::decode_bc1(info, pixels),
        TextureFormat::Dxt5 => bcn::decode_bc3(info, pixels),
        TextureFormat::Ati1n => bcn::decode_bc4(info, pixels),
        TextureFormat::Ati2n => bcn::decode_bc5(info, pixels),
        TextureFormat::Bc6h => bcn::decode_bc6h(info, pixels),
        TextureFormat::Bc7 => bcn::decode_bc7(info, pixels),
        TextureFormat::PngRgba8888 | TextureFormat::PngDxt5 | TextureFormat::JpegDxt5 => {
            inline::decode_inline(info, pixels)
        }
        other => Err(DecodeError::Unimplemented(other)),
    }
}
