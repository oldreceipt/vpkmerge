//! Splice an edited image back into an existing `.vtex_c` resource.
//!
//! The Phase 1 splice is intentionally narrow: it replaces exactly one
//! face/mip with newly-encoded bytes of the same length. It does not
//! regenerate the rest of the mip chain, does not change format, and does
//! not change dimensions. Those expansions are Phase 2/3 once the
//! compressed-format encoders land and we have somewhere to put the
//! "resize + Lanczos + re-encode each level" loop.
//!
//! The address arithmetic lives in [`crate::texture::face_mip_byte_range`];
//! this module just calls it, validates the new payload's length, and
//! splices.

use crate::error::EncodeError;
use crate::resource::Resource;
use crate::texture::{face_mip_byte_range, parse_texture_header};
use crate::DecodeOptions;

/// Replace one face/mip of an existing `.vtex_c` with `new_pixels`.
///
/// `new_pixels` must already be in the on-wire format of the texture (see
/// [`crate::encode_image`]) and must have exactly the same byte length as
/// the slot it's replacing. Returns a fresh owned copy of the resource with
/// the splice applied; the input is not mutated.
pub fn replace_face_mip(
    resource_bytes: &[u8],
    opts: DecodeOptions,
    new_pixels: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    let resource = Resource::parse(resource_bytes)?;
    let data = resource.data_block()?;
    let info = parse_texture_header(data)?;
    let range = face_mip_byte_range(&resource, &info, opts)?;
    let expected = range.end - range.start;
    if new_pixels.len() != expected {
        return Err(EncodeError::SpliceLengthMismatch {
            expected,
            got: new_pixels.len(),
        });
    }
    let mut out = resource_bytes.to_vec();
    out[range].copy_from_slice(new_pixels);
    Ok(out)
}

/// Convenience for the dominant case: mip 0, face 0, slice 0.
pub fn replace_face0_mip0(
    resource_bytes: &[u8],
    new_pixels: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    replace_face_mip(resource_bytes, DecodeOptions::default(), new_pixels)
}
