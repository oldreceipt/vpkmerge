//! Build the Foundry texture / icon browse index from a Deadlock VPK.
//!
//! The voice-line index ([`crate::catalog`]) is the browse backbone for the
//! sound picker; this is its visual counterpart for the Texture and Item Foundry
//! tabs. [`build_texture_index`] enumerates every `.vtex_c` entry and classifies
//! it (ability icon, item icon, hero portrait, hero skin texture, ability VFX)
//! purely from its path, so the full ~12.5K-entry index builds instantly with no
//! byte reads. [`thumbnail_png`] then decodes one entry to a small PNG on demand
//! (the same morphic decode path the recolor preview uses), and
//! [`cache_texture_thumbnails`] batches that into an on-disk thumbnail set + a
//! manifest a UI can render as a grid.
//!
//! Path taxonomy is grounded in the live `citadel/pak01`:
//! - ability icons:  `panorama/images/hud/abilities/<hero?>/...`
//! - item icons:     `panorama/images/items/`, `.../upgrades/mods_*`, `.../shop/`
//! - hero portraits: `panorama/images/heroes/`
//! - hero skins:     `models/heroes_staging|heroes_wip|heroes/<hero>/...` (the
//!   reskin / recolor targets)
//! - ability VFX:    `materials/particle/abilities/<hero>/...`
//!
//! Hero is the *codename* (`archer`, `astro`, ...), read from the path segment
//! that encodes it, matching every other tool in this crate.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use morphic::{DecodeOptions, Image, ImageData};

/// How a `.vtex_c` is used, inferred from its entry path. Drives which Foundry
/// tab browses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureCategory {
    /// `panorama/images/hud/abilities/` HUD ability icons.
    AbilityIcon,
    /// `panorama/images/items/`, `.../upgrades/mods_*`, `.../shop/` item +
    /// upgrade shop icons.
    ItemIcon,
    /// `panorama/images/heroes/` hero portraits / cards.
    HeroImage,
    /// `models/heroes_*/<hero>/` hero model textures: the skin / reskin targets.
    HeroModel,
    /// `materials/particle/abilities/<hero>/` ability VFX textures (recolor
    /// targets).
    AbilityVfx,
    /// Anything else (world materials, props, dev textures, ...).
    Other,
}

impl TextureCategory {
    /// Stable lower-kebab id for JSON / CLI filtering.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::AbilityIcon => "ability-icon",
            Self::ItemIcon => "item-icon",
            Self::HeroImage => "hero-image",
            Self::HeroModel => "hero-model",
            Self::AbilityVfx => "ability-vfx",
            Self::Other => "other",
        }
    }

    /// Parse a [`Self::id`] back into a category.
    #[must_use]
    pub fn from_id(s: &str) -> Option<Self> {
        Some(match s {
            "ability-icon" => Self::AbilityIcon,
            "item-icon" => Self::ItemIcon,
            "hero-image" => Self::HeroImage,
            "hero-model" => Self::HeroModel,
            "ability-vfx" => Self::AbilityVfx,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

/// One browseable texture, classified from its path. No pixels are read to build
/// this; the thumbnail is fetched separately via [`thumbnail_png`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureEntry {
    /// VPK entry path, e.g. `panorama/images/hud/abilities/astro/shotgun_psd.vtex_c`.
    /// Usable verbatim as the icon-swap / recolor target.
    pub path: String,
    /// Inferred use.
    pub category: TextureCategory,
    /// Hero codename the path encodes (`astro`, `archer`, ...), or `None` for
    /// shared / non-hero art.
    pub hero: Option<String>,
    /// Human-readable name derived from the filename, e.g. `"shotgun"` or
    /// `"bow color"`. The search key.
    pub label: String,
}

/// Build the texture browse index for `vpk_path`: one [`TextureEntry`] per
/// `.vtex_c`, classified and labeled from its path. Sorted by category then path
/// for a stable index. Cheap: it touches the VPK directory only, no file bodies.
pub fn build_texture_index(vpk_path: impl AsRef<Path>) -> Result<Vec<TextureEntry>> {
    let vpk_path = vpk_path.as_ref();
    let vpk =
        valve_pak::open(vpk_path).with_context(|| format!("opening {}", vpk_path.display()))?;

    let mut entries: Vec<TextureEntry> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vtex_c"))
        .map(|p| classify_texture(p))
        .collect();

    entries.sort_by(|a, b| {
        a.category
            .id()
            .cmp(b.category.id())
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(entries)
}

/// Classify a single `.vtex_c` entry path into a [`TextureEntry`].
#[must_use]
pub fn classify_texture(path: &str) -> TextureEntry {
    let (category, hero) = if let Some(rest) = path.strip_prefix("panorama/images/hud/abilities/") {
        (TextureCategory::AbilityIcon, first_dir(rest))
    } else if path.starts_with("panorama/images/items/")
        || path.starts_with("panorama/images/upgrades/mods_")
        || path.starts_with("panorama/images/shop/")
    {
        (TextureCategory::ItemIcon, None)
    } else if path.starts_with("panorama/images/heroes/") {
        // Hero portraits are flat files named `<codename>_card_psd.vtex_c` etc.;
        // the leading filename token is the codename.
        (TextureCategory::HeroImage, leading_token(path))
    } else if let Some(rest) = path
        .strip_prefix("models/heroes_staging/")
        .or_else(|| path.strip_prefix("models/heroes_wip/"))
        .or_else(|| path.strip_prefix("models/heroes/"))
    {
        (TextureCategory::HeroModel, first_dir(rest))
    } else if let Some(rest) = path.strip_prefix("materials/particle/abilities/") {
        (TextureCategory::AbilityVfx, first_dir(rest))
    } else {
        (TextureCategory::Other, None)
    };

    let label = texture_label(path, hero.as_deref());
    TextureEntry {
        path: path.to_owned(),
        category,
        hero,
        label,
    }
}

/// First path segment of `rest` if it is a directory (there is a `/` after it),
/// else `None`. `"astro/shotgun_psd.vtex_c"` -> `Some("astro")`;
/// `"ability_activate_psd.vtex_c"` -> `None`.
fn first_dir(rest: &str) -> Option<String> {
    rest.split_once('/').map(|(head, _)| head.to_owned())
}

/// Leading `_`-delimited token of a path's filename (the hero codename for the
/// flat hero-portrait files). `"panorama/images/heroes/archer_card_psd.vtex_c"`
/// -> `Some("archer")`.
fn leading_token(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let token = file.split('_').next()?;
    if token.is_empty() {
        None
    } else {
        Some(token.to_owned())
    }
}

/// Image-format / source tokens that trail a Source 2 texture filename before
/// the content hash. Stripped from the label.
const FORMAT_TOKENS: &[&str] = &["psd", "png", "jpg", "jpeg", "tga", "exr", "vtex"];

/// Derive a searchable label from a `.vtex_c` filename: drop the `.vtex_c`
/// extension, a trailing 8-hex content hash, a trailing format token (`_psd`,
/// `_png`, ...), and the hero codename prefix, then spell underscores as spaces.
/// `"panorama/images/hud/abilities/astro/shotgun_psd.vtex_c"` with hero `astro`
/// stays `"shotgun"` (no hero prefix on this one); `"archer_bow_color_png_<hash>"`
/// with hero `archer` -> `"bow color"`.
fn texture_label(path: &str, hero: Option<&str>) -> String {
    let mut stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .strip_suffix(".vtex_c")
        .unwrap_or(path);

    // Trailing 8-hex content hash (Source 2 renames recompiled textures).
    if let Some((head, tail)) = stem.rsplit_once('_') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_hexdigit()) {
            stem = head;
        }
    }
    // Trailing format token.
    if let Some((head, tail)) = stem.rsplit_once('_') {
        if FORMAT_TOKENS.contains(&tail) {
            stem = head;
        }
    }
    // Hero codename prefix (redundant with the `hero` field).
    if let Some(h) = hero {
        if let Some(rest) = stem.strip_prefix(h) {
            stem = rest.trim_start_matches('_');
        }
    }
    stem.replace('_', " ").trim().to_owned()
}

/// A decoded thumbnail PNG plus the dimensions involved.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// PNG bytes (RGBA8), at most `max_edge` on the longer side.
    pub png: Vec<u8>,
    /// Thumbnail width / height.
    pub width: u32,
    pub height: u32,
    /// Full-resolution source dimensions (mip 0).
    pub source_width: u32,
    pub source_height: u32,
    /// VRF texture format name (e.g. `Bc7`).
    pub format: String,
}

/// Decode a `.vtex_c` to a thumbnail PNG no larger than `max_edge` on its longer
/// side, preserving aspect ratio.
///
/// To keep decode cheap on 4K source textures, this decodes the smallest mip
/// whose longer edge is still at least `max_edge` (the mip chain does the bulk of
/// the downscale for free), then box-filters that mip down to the exact target.
/// HDR (f16) sources are clamped to `[0,1]` and treated as linear; the browse
/// icons this targets are all LDR.
pub fn thumbnail_png(vtex_bytes: &[u8], max_edge: u32) -> Result<Thumbnail> {
    let info = morphic::inspect(vtex_bytes).context("reading texture header")?;
    let max_edge = max_edge.max(1);
    let mip = choose_mip(
        u32::from(info.width),
        u32::from(info.height),
        info.mip_count,
        max_edge,
    );
    let image = morphic::decode_at(
        vtex_bytes,
        &DecodeOptions {
            mip,
            slice: 0,
            face: 0,
        },
    )
    .context("decoding texture mip for thumbnail")?;

    let (sw, sh) = (image.width.max(1), image.height.max(1));
    let rgba = to_rgba8(&image);
    let src = image::RgbaImage::from_raw(sw, sh, rgba)
        .context("thumbnail source buffer size mismatch")?;

    let (tw, th) = fit_within(sw, sh, max_edge);
    let resized = image::imageops::resize(&src, tw, th, image::imageops::FilterType::Triangle);

    let mut png = Vec::new();
    {
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(resized.as_raw(), tw, th, image::ExtendedColorType::Rgba8)
            .context("encoding thumbnail PNG")?;
    }

    Ok(Thumbnail {
        png,
        width: tw,
        height: th,
        source_width: u32::from(info.width),
        source_height: u32::from(info.height),
        format: format!("{:?}", info.format),
    })
}

/// Choose the smallest mip index whose longer edge is still at least `max_edge`,
/// so the decode does as little work as possible while leaving enough resolution
/// for a clean downscale. Clamped to the available mip count.
fn choose_mip(width: u32, height: u32, mip_count: u8, max_edge: u32) -> u8 {
    let last = mip_count.saturating_sub(1);
    let mut chosen = 0u8;
    for m in 0..=last {
        let edge = (width >> m).max(1).max((height >> m).max(1));
        if edge >= max_edge {
            chosen = m;
        } else {
            break;
        }
    }
    chosen.min(last)
}

/// Target dimensions that fit `(w, h)` within a `max_edge` box, preserving aspect
/// and never upscaling.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn fit_within(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let longer = w.max(h);
    if longer <= max_edge {
        return (w, h);
    }
    let scale = f64::from(max_edge) / f64::from(longer);
    // Both products are non-negative and well under u32::MAX (max_edge bounds them).
    let tw = ((f64::from(w) * scale).round() as u32).max(1);
    let th = ((f64::from(h) * scale).round() as u32).max(1);
    (tw, th)
}

/// Flatten a decoded morphic [`Image`] to tightly-packed RGBA8 bytes.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_rgba8(image: &Image) -> Vec<u8> {
    match &image.data {
        ImageData::Rgba8(bytes) => bytes.clone(),
        // Clamped to [0,1] then scaled to [0,255]: always a valid u8.
        ImageData::Rgba16F(halfs) => halfs
            .iter()
            .map(|h| (h.to_f32().clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect(),
    }
}

/// One row of a written thumbnail set: the source entry, the PNG filename
/// emitted (relative to the output dir), and the dimensions.
#[derive(Debug, Clone)]
pub struct CachedThumbnail {
    pub entry: String,
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub format: String,
}

/// Outcome of caching one entry's thumbnail: the manifest row, or the entry path
/// plus why it was skipped (a texture that fails to decode never sinks the batch).
#[derive(Debug, Clone)]
pub enum ThumbnailOutcome {
    Cached(CachedThumbnail),
    Skipped { entry: String, reason: String },
}

/// Decode + write a PNG thumbnail for each of `entries` into `out_dir`, returning
/// a per-entry outcome. The directory is created if needed. Filenames are the
/// entry path with `/` mapped to `__` and the extension changed to `.png`, so
/// they stay unique and reversible. A decode failure on one texture is reported
/// as [`ThumbnailOutcome::Skipped`], not a hard error.
pub fn cache_texture_thumbnails(
    vpk_path: impl AsRef<Path>,
    entries: &[TextureEntry],
    out_dir: impl AsRef<Path>,
    max_edge: u32,
) -> Result<Vec<ThumbnailOutcome>> {
    let vpk_path = vpk_path.as_ref();
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating thumbnail dir {}", out_dir.display()))?;

    let vpk =
        valve_pak::open(vpk_path).with_context(|| format!("opening {}", vpk_path.display()))?;

    let mut outcomes = Vec::with_capacity(entries.len());
    for entry in entries {
        match cache_one(&vpk, &entry.path, out_dir, max_edge) {
            Ok(cached) => outcomes.push(ThumbnailOutcome::Cached(cached)),
            Err(e) => outcomes.push(ThumbnailOutcome::Skipped {
                entry: entry.path.clone(),
                reason: format!("{e:#}"),
            }),
        }
    }
    Ok(outcomes)
}

fn cache_one(
    vpk: &valve_pak::VPK,
    entry: &str,
    out_dir: &Path,
    max_edge: u32,
) -> Result<CachedThumbnail> {
    let mut file = vpk
        .get_file(entry)
        .with_context(|| format!("locating {entry}"))?;
    let bytes = file
        .read_all()
        .with_context(|| format!("reading {entry}"))?;
    let thumb = thumbnail_png(&bytes, max_edge)?;

    let file_name = thumbnail_file_name(entry);
    let dest: PathBuf = out_dir.join(&file_name);
    std::fs::write(&dest, &thumb.png).with_context(|| format!("writing {}", dest.display()))?;

    Ok(CachedThumbnail {
        entry: entry.to_owned(),
        file: file_name,
        width: thumb.width,
        height: thumb.height,
        source_width: thumb.source_width,
        source_height: thumb.source_height,
        format: thumb.format,
    })
}

/// Map a VPK entry path to a flat, unique PNG filename:
/// `panorama/images/hud/abilities/astro/shotgun_psd.vtex_c`
/// -> `panorama__images__hud__abilities__astro__shotgun_psd.png`.
#[must_use]
pub fn thumbnail_file_name(entry: &str) -> String {
    let stem = entry.strip_suffix(".vtex_c").unwrap_or(entry);
    format!("{}.png", stem.replace('/', "__"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ability_icon_with_hero() {
        let e = classify_texture("panorama/images/hud/abilities/astro/shotgun_psd.vtex_c");
        assert_eq!(e.category, TextureCategory::AbilityIcon);
        assert_eq!(e.hero.as_deref(), Some("astro"));
        assert_eq!(e.label, "shotgun");
    }

    #[test]
    fn shared_ability_icon_has_no_hero() {
        let e = classify_texture("panorama/images/hud/abilities/ability_activate_psd.vtex_c");
        assert_eq!(e.category, TextureCategory::AbilityIcon);
        assert_eq!(e.hero, None);
        assert_eq!(e.label, "ability activate");
    }

    #[test]
    fn classifies_item_and_upgrade_icons() {
        assert_eq!(
            classify_texture("panorama/images/items/brawl/apex_combat_psd.vtex_c").category,
            TextureCategory::ItemIcon
        );
        assert_eq!(
            classify_texture("panorama/images/upgrades/mods_weapon/headshot_booster_psd.vtex_c")
                .category,
            TextureCategory::ItemIcon
        );
        assert_eq!(
            classify_texture("panorama/images/shop/catalog/foo_psd.vtex_c").category,
            TextureCategory::ItemIcon
        );
    }

    #[test]
    fn classifies_hero_portrait_and_codename() {
        let e = classify_texture("panorama/images/heroes/archer_card_psd.vtex_c");
        assert_eq!(e.category, TextureCategory::HeroImage);
        assert_eq!(e.hero.as_deref(), Some("archer"));
        assert_eq!(e.label, "card");
    }

    #[test]
    fn classifies_hero_model_texture() {
        let e = classify_texture(
            "models/heroes_staging/archer/bow/materials/archer_bow_color_png_b68c2251.vtex_c",
        );
        assert_eq!(e.category, TextureCategory::HeroModel);
        assert_eq!(e.hero.as_deref(), Some("archer"));
        assert_eq!(e.label, "bow color");
    }

    #[test]
    fn classifies_ability_vfx() {
        let e = classify_texture("materials/particle/abilities/abrams/abrams_mystic.vtex_c");
        assert_eq!(e.category, TextureCategory::AbilityVfx);
        assert_eq!(e.hero.as_deref(), Some("abrams"));
        assert_eq!(e.label, "mystic");
    }

    #[test]
    fn world_textures_are_other() {
        assert_eq!(
            classify_texture("materials/brick/brick_01_color_tga_1234abcd.vtex_c").category,
            TextureCategory::Other
        );
    }

    #[test]
    fn category_id_round_trips() {
        for c in [
            TextureCategory::AbilityIcon,
            TextureCategory::ItemIcon,
            TextureCategory::HeroImage,
            TextureCategory::HeroModel,
            TextureCategory::AbilityVfx,
            TextureCategory::Other,
        ] {
            assert_eq!(TextureCategory::from_id(c.id()), Some(c));
        }
        assert_eq!(TextureCategory::from_id("nope"), None);
    }

    #[test]
    fn mip_choice_prefers_smallest_sufficient() {
        // 4096x4096, 13 mips, target 256: mip 4 is 256, the smallest >= 256.
        assert_eq!(choose_mip(4096, 4096, 13, 256), 4);
        // Target larger than the texture: mip 0.
        assert_eq!(choose_mip(128, 128, 8, 256), 0);
        // Single-mip (inline) texture: always mip 0.
        assert_eq!(choose_mip(512, 512, 1, 64), 0);
    }

    #[test]
    fn fit_preserves_aspect_and_never_upscales() {
        assert_eq!(fit_within(4096, 2048, 256), (256, 128));
        assert_eq!(fit_within(100, 200, 256), (100, 200));
        assert_eq!(fit_within(256, 256, 256), (256, 256));
    }

    #[test]
    fn thumbnail_file_name_is_flat_and_unique() {
        assert_eq!(
            thumbnail_file_name("panorama/images/hud/abilities/astro/shotgun_psd.vtex_c"),
            "panorama__images__hud__abilities__astro__shotgun_psd.png"
        );
    }
}
