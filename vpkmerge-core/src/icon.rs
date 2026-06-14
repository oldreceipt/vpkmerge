//! Build a custom Deadlock icon / hero-card texture from a user PNG.
//!
//! Deadlock hero card art ships as `.vtex_c` under
//! `panorama/images/heroes/<codename>_<variant>_(psd or png).vtex_c`, each variant
//! at its own fixed dimensions (minimap, small, card, `card_critical`, `card_gloat`,
//! vertical). To let a user drop in their own art without an encoder that has to
//! reproduce the exact header/format the game expects, we reuse an existing
//! texture as a *template*: take the base game's variant `.vtex_c`, decode the
//! user PNG, resize it to the template's mip-0 dimensions, and splice it into the
//! template's mip chain via [`morphic::replace_mip_chain`] (the same in-place
//! mechanism the ability-VFX recolor uses, see `recolor.rs`).
//!
//! Packing the result at the template's own entry path overrides the base
//! texture in place: no `.vmat_c` edit, format and header preserved, so the game
//! loads it exactly as it would the original.

use anyhow::{Context, Result};
use morphic::{Image, ImageData};

/// Decode `png_bytes`, resize to `width` x `height`, and wrap as a morphic
/// RGBA8 [`Image`] (row-major, top-left origin) ready for mip splicing.
pub fn png_to_rgba8_image(png_bytes: &[u8], width: u32, height: u32) -> Result<Image> {
    let decoded = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .context("decoding PNG (input must be a valid PNG)")?
        .to_rgba8();
    // Lanczos3 keeps card art crisp when down/upscaling to the template dims.
    let resized = image::imageops::resize(
        &decoded,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    Ok(Image {
        width,
        height,
        data: ImageData::Rgba8(resized.into_raw()),
    })
}

/// Build a new `.vtex_c` by replacing `template_vtex`'s image with `png_bytes`
/// (resized to the template's dimensions), preserving the template's format,
/// header, and mip count. The returned bytes pack back at the template's entry
/// path to override the base texture in place.
pub fn build_icon_from_template(template_vtex: &[u8], png_bytes: &[u8]) -> Result<Vec<u8>> {
    let info = morphic::inspect(template_vtex).context("reading template .vtex_c header")?;
    // BCn / 8-bit card formats decode to RGBA8; HDR (Rgba16F) templates are not
    // a hero-card art path and would need float pixels, so reject them clearly.
    if matches!(
        info.format,
        morphic::TextureFormat::Bc6h | morphic::TextureFormat::Rgba16161616F
    ) {
        anyhow::bail!(
            "template is an HDR ({:?}) texture; custom PNG import supports 8-bit card formats only",
            info.format
        );
    }
    let image = png_to_rgba8_image(png_bytes, u32::from(info.width), u32::from(info.height))?;
    morphic::replace_mip_chain(template_vtex, &image)
        .context("splicing the PNG into the template's mip chain")
}
