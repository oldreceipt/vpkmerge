use base64::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Cursor;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
struct ModInfo {
    path: String,
    name: String,
    file_count: usize,
    size_bytes: u64,
    file_paths: Vec<String>,
}

impl From<vpkmerge_core::ModInfo> for ModInfo {
    fn from(m: vpkmerge_core::ModInfo) -> Self {
        ModInfo {
            path: m.path.to_string_lossy().into_owned(),
            name: m.name,
            file_count: m.file_count,
            size_bytes: m.size_bytes,
            file_paths: m.file_paths,
        }
    }
}

#[derive(Serialize)]
struct Conflict {
    path: String,
    owner_indices: Vec<usize>,
}

impl From<vpkmerge_core::Conflict> for Conflict {
    fn from(c: vpkmerge_core::Conflict) -> Self {
        Conflict {
            path: c.path,
            owner_indices: c.owner_indices,
        }
    }
}

#[derive(Serialize)]
struct MergeReport {
    total_entries: usize,
    overridden: usize,
    inputs: usize,
    output_path: String,
}

impl From<vpkmerge_core::MergeReport> for MergeReport {
    fn from(r: vpkmerge_core::MergeReport) -> Self {
        MergeReport {
            total_entries: r.total_entries,
            overridden: r.overridden_paths,
            inputs: r.inputs,
            output_path: r.output_path.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Serialize)]
struct HeroOption {
    codename: String,
    label: String,
    particles: usize,
    textures: usize,
    materials: usize,
    models: usize,
    has_preview: bool,
}

#[derive(Serialize)]
struct HeroPrismReport {
    codename: String,
    particles_total: usize,
    particles_recolored: usize,
    particles_no_color: usize,
    particles_unpatchable: usize,
    gradient_fields: usize,
    color_fields: usize,
    boosted_fields: usize,
    lifted_black_gradient_fields: usize,
    random_range_fields: usize,
    textures_recolored: usize,
    materials_recolored: usize,
    materials_unpatchable: usize,
    models_recolored: usize,
    model_vertices: usize,
    particles_animated: usize,
    texture_age_inputs: usize,
    texture_offset_multipliers: usize,
    gradient_timing_edits: usize,
    total_entries: usize,
    output_path: String,
}

impl HeroPrismReport {
    fn from_core(report: vpkmerge_core::HeroPrismRecolorReport, output_path: String) -> Self {
        Self {
            codename: report.codename,
            particles_total: report.particles_total,
            particles_recolored: report.particles_recolored,
            particles_no_color: report.particles_no_color,
            particles_unpatchable: report.particles_unpatchable,
            gradient_fields: report.gradient_fields,
            color_fields: report.color_fields,
            boosted_fields: report.boosted_fields,
            lifted_black_gradient_fields: report.lifted_black_gradient_fields,
            random_range_fields: report.random_range_fields,
            textures_recolored: report.textures_recolored,
            materials_recolored: report.materials_recolored,
            materials_unpatchable: report.materials_unpatchable,
            models_recolored: report.models_recolored,
            model_vertices: report.model_vertices,
            particles_animated: report.particles_animated,
            texture_age_inputs: report.texture_age_inputs,
            texture_offset_multipliers: report.texture_offset_multipliers,
            gradient_timing_edits: report.gradient_timing_edits,
            total_entries: report.total_entries,
            output_path,
        }
    }
}

#[tauri::command]
async fn pick_vpk_files(app: AppHandle) -> Vec<String> {
    app.dialog()
        .file()
        .add_filter("VPK files", &["vpk"])
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
async fn pick_output_path(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .add_filter("VPK file", &["vpk"])
        .set_file_name("merged_dir.vpk")
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn add_mod(path: String) -> Result<ModInfo, String> {
    vpkmerge_core::inspect(&path)
        .map(Into::into)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn detect_conflicts(ordered_paths: Vec<String>) -> Result<Vec<Conflict>, String> {
    vpkmerge_core::detect_conflicts(&ordered_paths)
        .map(|cs| cs.into_iter().map(Into::into).collect())
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn merge_vpks(
    ordered_paths: Vec<String>,
    output_path: String,
    policy: Option<String>,
    overrides: Option<HashMap<String, usize>>,
) -> Result<MergeReport, String> {
    let collision_policy = match policy.as_deref() {
        None | Some("last_wins") => vpkmerge_core::CollisionPolicy::LastWins,
        Some("first_wins") => vpkmerge_core::CollisionPolicy::FirstWins,
        Some("strict") => vpkmerge_core::CollisionPolicy::Error,
        Some(other) => return Err(format!("unknown policy: {other}")),
    };
    vpkmerge_core::merge(
        &ordered_paths,
        &output_path,
        &vpkmerge_core::MergeOptions {
            collision_policy,
            overrides: overrides.unwrap_or_default(),
        },
    )
    .map(Into::into)
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

/// Best-effort resolution of the local Deadlock `pak01_dir.vpk`. Checks the
/// standard Steam install roots on Linux and Windows. Returns `None` if no
/// candidate exists; the frontend then falls back to asking the user.
#[tauri::command]
async fn default_deadlock_vpk_path() -> Option<String> {
    default_deadlock_vpk_candidate().map(|p| p.to_string_lossy().into_owned())
}

fn default_deadlock_vpk_candidate() -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(target_os = "linux") {
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(
                format!("{home}/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk")
                    .into(),
            );
            candidates.push(
                format!(
                    "{home}/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk"
                )
                .into(),
            );
        }
    } else if cfg!(target_os = "windows") {
        candidates.push(
            "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Deadlock\\game\\citadel\\pak01_dir.vpk"
                .into(),
        );
        candidates.push(
            "C:\\Program Files\\Steam\\steamapps\\common\\Deadlock\\game\\citadel\\pak01_dir.vpk"
                .into(),
        );
    }
    candidates.into_iter().find(|p| p.exists())
}

#[tauri::command]
async fn default_addon_output_path() -> Option<String> {
    let base = default_deadlock_vpk_candidate()?;
    let citadel_dir = base.parent()?;
    let addons = citadel_dir.join("addons");
    let mut max_slot = 1u32;
    if let Ok(read_dir) = std::fs::read_dir(&addons) {
        for entry in read_dir.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(slot) = name
                .strip_prefix("pak")
                .and_then(|s| s.strip_suffix("_dir.vpk"))
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            max_slot = max_slot.max(slot);
        }
    }
    Some(
        addons
            .join(format!("pak{:02}_dir.vpk", max_slot + 1))
            .to_string_lossy()
            .into_owned(),
    )
}

#[tauri::command]
async fn supported_hero_options() -> Vec<HeroOption> {
    vpkmerge_core::pinned_hero_codenames()
        .iter()
        .filter_map(|codename| {
            let recipe = vpkmerge_core::recipe_for(codename)?;
            Some(HeroOption {
                codename: (*codename).to_string(),
                label: hero_label(codename).to_string(),
                particles: recipe.particle_prefixes.len(),
                textures: recipe.texture_entries.len(),
                materials: recipe.material_entries.len(),
                models: recipe.model_entries.len(),
                has_preview: recipe.preview_texture.is_some(),
            })
        })
        .collect()
}

fn hero_label(codename: &str) -> &str {
    match codename {
        "bookworm" => "Paige",
        "necro" => "Graves",
        "inferno" => "Infernus",
        "yamato" => "Yamato",
        "unicorn" => "Celeste",
        "gigawatt" => "Seven",
        "vampirebat" => "Mina",
        "wraith" => "Wraith",
        _ => codename,
    }
}

/// Trims an optional `--base`-style path: blank/whitespace-only -> `None`.
fn opt_path(s: Option<String>) -> Option<std::path::PathBuf> {
    s.filter(|p| !p.trim().is_empty())
        .map(std::path::PathBuf::from)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri command: one arg per UI control.
async fn build_hero_prism_vpk(
    vpk_path: String,
    base_path: Option<String>,
    hero: String,
    animated: bool,
    hue_offset: f64,
    saturation: f64,
    brightness: f64,
    output_path: String,
) -> Result<HeroPrismReport, String> {
    let base = opt_path(base_path);
    let tuning = vpkmerge_core::PrismTuning {
        hue_offset,
        saturation,
        brightness,
    };
    let report = vpkmerge_core::prism_recolor_hero_to_addon_tuned(
        &vpk_path,
        base.as_deref(),
        &hero,
        animated,
        tuning,
        &output_path,
    )
    .map_err(|e| format!("{e:#}"))?;
    Ok(HeroPrismReport::from_core(report, output_path))
}

#[derive(Serialize)]
struct HeroRecolorReport {
    codename: String,
    hue: f64,
    saturation: f64,
    value: f64,
    particles_recolored: usize,
    particles_no_color: usize,
    particles_unpatchable: usize,
    textures_recolored: usize,
    materials_recolored: usize,
    materials_unpatchable: usize,
    models_recolored: usize,
    model_vertices: usize,
    total_entries: usize,
    output_path: String,
}

impl HeroRecolorReport {
    fn from_core(r: vpkmerge_core::HeroRecolorReport, output_path: String) -> Self {
        Self {
            codename: r.codename,
            hue: r.hue,
            saturation: r.saturation,
            value: r.value,
            particles_recolored: r.particles_recolored,
            particles_no_color: r.particles_no_color,
            particles_unpatchable: r.particles_unpatchable,
            textures_recolored: r.textures_recolored,
            materials_recolored: r.materials_recolored,
            materials_unpatchable: r.materials_unpatchable,
            models_recolored: r.models_recolored,
            model_vertices: r.model_vertices,
            total_entries: r.total_entries,
            output_path,
        }
    }
}

/// Recolor a hero's whole ability-VFX set to one absolute hue (the solid-color
/// sibling of the prism). `brightness` maps to HSV value.
#[tauri::command]
async fn recolor_hero_vpk(
    vpk_path: String,
    base_path: Option<String>,
    hero: String,
    hue: f64,
    saturation: f64,
    brightness: f64,
    output_path: String,
) -> Result<HeroRecolorReport, String> {
    let base = opt_path(base_path);
    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);
    let report =
        vpkmerge_core::recolor_hero_to_addon(&vpk_path, base.as_deref(), &hero, recolor, &output_path)
            .map_err(|e| format!("{e:#}"))?;
    Ok(HeroRecolorReport::from_core(report, output_path))
}

/// Fast solid-hue swatch of the hero recipe's representative texture (no bake).
/// Errors for particle-only heroes (no preview texture); the caller hides the
/// swatch up front using `HeroOption.has_preview`.
#[tauri::command]
async fn recolor_hero_preview(
    vpk_path: String,
    base_path: Option<String>,
    hero: String,
    hue: f64,
    saturation: f64,
    brightness: f64,
) -> Result<String, String> {
    let base = opt_path(base_path);
    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);
    let png = vpkmerge_core::recolor_hero_preview_png(&vpk_path, base.as_deref(), &hero, recolor)
        .map_err(|e| format!("{e:#}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{b64}"))
}

#[derive(Serialize)]
struct HeroRainbowReport {
    codename: String,
    particles_total: usize,
    particles_patchable: usize,
    particles_color_free: usize,
    particles_unpatchable: usize,
    visible_color_fields: usize,
    gradient_fields: usize,
    multi_stop_gradient_fields: usize,
    age_gradient_fields: usize,
    looped_gradient_fields: usize,
    random_color_initializers: usize,
    color_interpolate_ops: usize,
    texture_entries: usize,
    material_entries: usize,
    model_entries: usize,
    /// Suitability tier, matching the CLI `rainbow-scan` (`looped` > `animated`
    /// > `strong` > `gradient` > `static`, or `none`).
    mode: String,
}

/// Mirrors the CLI's `rainbow_scan_mode` so the GUI shows the same verdict.
fn rainbow_mode(r: &vpkmerge_core::HeroRainbowSupportReport) -> &'static str {
    if r.particles_patchable == 0 {
        "none"
    } else if r.looped_gradient_fields > 0 {
        "looped"
    } else if r.collection_age_gradient_fields + r.particle_age_gradient_fields > 0 {
        "animated"
    } else if r.multi_stop_gradient_fields >= 12 {
        "strong"
    } else if r.gradient_fields > 0 {
        "gradient"
    } else {
        "static"
    }
}

/// Scan one hero recipe for rainbow/animated-rainbow suitability (the GUI's
/// equivalent of `vpkmerge rainbow-scan --hero <CODENAME>`).
#[tauri::command]
async fn scan_hero_rainbow(
    vpk_path: String,
    base_path: Option<String>,
    hero: String,
) -> Result<HeroRainbowReport, String> {
    let base = opt_path(base_path);
    let r = vpkmerge_core::scan_hero_rainbow_support(&vpk_path, base.as_deref(), &hero)
        .map_err(|e| format!("{e:#}"))?;
    let mode = rainbow_mode(&r).to_string();
    Ok(HeroRainbowReport {
        codename: r.codename,
        particles_total: r.particles_total,
        particles_patchable: r.particles_patchable,
        particles_color_free: r.particles_color_free,
        particles_unpatchable: r.particles_unpatchable + r.particles_decode_failed,
        visible_color_fields: r.visible_color_fields,
        gradient_fields: r.gradient_fields,
        multi_stop_gradient_fields: r.multi_stop_gradient_fields,
        age_gradient_fields: r.collection_age_gradient_fields + r.particle_age_gradient_fields,
        looped_gradient_fields: r.looped_gradient_fields,
        random_color_initializers: r.random_color_initializers,
        color_interpolate_ops: r.color_interpolate_ops,
        texture_entries: r.texture_entries,
        material_entries: r.material_entries,
        model_entries: r.model_entries,
        mode,
    })
}

#[derive(Serialize)]
struct TexturePreview {
    /// `data:image/png;base64,...` URL, ready to drop into `<img :src>`.
    data_url: String,
    /// Displayed (post-downscale) dimensions.
    width: u32,
    height: u32,
    /// Original texture dimensions before any downscale.
    orig_width: u32,
    orig_height: u32,
    /// `VTexFormat` name (e.g. "BC7", "RGBA8888").
    format: String,
    /// True if the texture has the `CUBE_TEXTURE` flag; callers may then
    /// re-invoke with `face` in `1..=5` to see other faces. Face ordering
    /// is `[+X, -X, +Y, -Y, +Z, -Z]`; default (no face arg) is `+X`.
    is_cubemap: bool,
    /// Number of mip levels the source texture has. Callers may pass `mip`
    /// in `0..mip_count` to see lower-detail versions.
    mip_count: u8,
}

/// A decoded `morphic::Image` -> a (downscaled) PNG data URL plus its displayed
/// and original dimensions. Shared by `preview_texture` and the live
/// `preview_texture_recolor`. HDR (f16) is tone-mapped via
/// [`tonemap_rgba_f16_to_u8`]; LDR passes straight through.
fn image_to_png_data_url(
    img: morphic::Image,
    cap: u32,
) -> Result<(String, u32, u32, u32, u32), String> {
    let (orig_w, orig_h) = (img.width, img.height);
    let raw_rgba = match img.data {
        morphic::ImageData::Rgba8(buf) => buf,
        morphic::ImageData::Rgba16F(buf) => tonemap_rgba_f16_to_u8(&buf),
    };
    let buffer: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::from_raw(orig_w, orig_h, raw_rgba)
            .ok_or_else(|| "decoded buffer size mismatch".to_string())?;
    let dyn_img = image::DynamicImage::ImageRgba8(buffer);
    let downscaled = if orig_w > cap || orig_h > cap {
        dyn_img.resize(cap, cap, image::imageops::FilterType::Triangle)
    } else {
        dyn_img
    };
    let (w, h) = (downscaled.width(), downscaled.height());
    let mut png_bytes: Vec<u8> = Vec::new();
    downscaled
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("png encode: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok((format!("data:image/png;base64,{b64}"), w, h, orig_w, orig_h))
}

#[tauri::command]
async fn preview_texture(
    vpk_path: String,
    entry: String,
    max_dim: Option<u32>,
    face: Option<u8>,
    mip: Option<u8>,
) -> Result<TexturePreview, String> {
    let cap = max_dim.unwrap_or(256).max(16);
    let face = face.unwrap_or(0);
    let mip = mip.unwrap_or(0);
    let vpk = valve_pak::open(&vpk_path).map_err(|e| format!("open vpk: {e}"))?;
    let mut vf = vpk
        .get_file(&entry)
        .map_err(|e| format!("entry not found: {e}"))?;
    let bytes = vf.read_all().map_err(|e| format!("read entry: {e}"))?;

    let info = morphic::inspect(&bytes).map_err(|e| format!("inspect: {e}"))?;
    let is_cubemap = info.flags.contains(morphic::TextureFlags::CUBE_TEXTURE);
    let mip_count = info.mip_count;
    let img = match morphic::decode_at(
        &bytes,
        &morphic::DecodeOptions {
            mip,
            slice: 0,
            face,
        },
    ) {
        Ok(img) => img,
        Err(morphic::DecodeError::Unimplemented(fmt)) => {
            return Err(format!("preview not supported for format: {}", fmt.name()));
        }
        Err(e) => return Err(format!("decode: {e}")),
    };

    let (data_url, w, h, orig_w, orig_h) = image_to_png_data_url(img, cap)?;
    Ok(TexturePreview {
        data_url,
        width: w,
        height: h,
        orig_width: orig_w,
        orig_height: orig_h,
        format: info.format.name().to_string(),
        is_cubemap,
        mip_count,
    })
}

// Reinhard tone-map + sRGB-ish 1/2.2 gamma so HDR (BC6H, future RGBA16F)
// textures look like reasonable LDR previews in the conflict modal. Negative
// values are clamped to 0; alpha is a direct clamp to [0, 1]. This matches
// the spirit of what VRF's tone-mapped PNG output produces for the same
// fixtures, though it's not bit-exact with VRF (which is fine, this is
// purely for human preview).
fn tonemap_rgba_f16_to_u8(buf: &[half::f16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len());
    for px in buf.chunks_exact(4) {
        for c in &px[..3] {
            let v = c.to_f32().max(0.0);
            let tonemapped = v / (1.0 + v);
            let gamma = tonemapped.powf(1.0 / 2.2);
            out.push(float_to_u8(gamma));
        }
        out.push(float_to_u8(px[3].to_f32()));
    }
    out
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn float_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> [half::f16; 4] {
        [
            half::f16::from_f32(r),
            half::f16::from_f32(g),
            half::f16::from_f32(b),
            half::f16::from_f32(a),
        ]
    }

    #[test]
    fn tonemap_black_stays_black() {
        let out = tonemap_rgba_f16_to_u8(&rgba(0.0, 0.0, 0.0, 1.0));
        assert_eq!(out, vec![0, 0, 0, 255]);
    }

    #[test]
    fn tonemap_alpha_is_direct_clamp() {
        // Color channels tone-map (so 0.0 -> 0), alpha passes straight through.
        let out = tonemap_rgba_f16_to_u8(&rgba(0.0, 0.0, 0.0, 0.5));
        assert_eq!(out[3], 128); // round(0.5 * 255) = 128
        let saturated = tonemap_rgba_f16_to_u8(&rgba(0.0, 0.0, 0.0, 4.0));
        assert_eq!(saturated[3], 255);
        let neg_alpha = tonemap_rgba_f16_to_u8(&rgba(0.0, 0.0, 0.0, -1.0));
        assert_eq!(neg_alpha[3], 0);
    }

    #[test]
    fn tonemap_compresses_bright_values_below_white() {
        // Even a "100x brighter than mid-grey" input must not produce pure
        // white, that's the whole point of Reinhard: 100 / (1 + 100) ≈ 0.99,
        // then gamma 1/2.2 ≈ 0.995, * 255 ≈ 253.
        let out = tonemap_rgba_f16_to_u8(&rgba(100.0, 100.0, 100.0, 1.0));
        assert!(
            out[0] >= 250 && out[0] < 255,
            "bright but not saturated, got {}",
            out[0]
        );
        assert_eq!(out[3], 255);
    }

    #[test]
    fn tonemap_negative_color_clamps_to_zero() {
        let out = tonemap_rgba_f16_to_u8(&rgba(-1.0, -0.5, -10.0, 1.0));
        assert_eq!(&out[..3], &[0, 0, 0]);
    }

    #[test]
    fn tonemap_mid_grey_is_reasonable() {
        // 0.5 -> 0.5/1.5 = 0.333... -> ^(1/2.2) ≈ 0.605 -> *255 ≈ 154
        let out = tonemap_rgba_f16_to_u8(&rgba(0.5, 0.5, 0.5, 1.0));
        for c in &out[..3] {
            assert!(*c >= 150 && *c <= 158, "mid-grey ~154, got {c}");
        }
    }
}

/// Generic result for the single-asset addon bakes (texture / model /
/// soundevents): where it was written and a one-line human summary.
#[derive(Serialize)]
struct AddonResult {
    output_path: String,
    entries: usize,
    summary: String,
}

/// Live preview of a `.vtex_c` recolor (the design-intent color, before the
/// lossy re-encode). Reads the entry, recolors the top mip in memory, returns a
/// downscaled PNG data URL. `brightness` maps to HSV value.
#[tauri::command]
async fn preview_texture_recolor(
    vpk_path: String,
    entry: String,
    hue: f64,
    saturation: f64,
    brightness: f64,
    max_dim: Option<u32>,
) -> Result<String, String> {
    let cap = max_dim.unwrap_or(256).max(16);
    let bytes = vpkmerge_core::read_vpk_entry(&vpk_path, &entry).map_err(|e| format!("{e:#}"))?;
    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);
    let img =
        vpkmerge_core::recolor_texture_image(&bytes, recolor).map_err(|e| format!("{e:#}"))?;
    let (data_url, ..) = image_to_png_data_url(img, cap)?;
    Ok(data_url)
}

/// Recolor a single `.vtex_c` and pack it into an addon VPK at its own entry
/// path, overriding the base texture in place.
#[tauri::command]
async fn recolor_texture_to_addon(
    vpk_path: String,
    entry: String,
    hue: f64,
    saturation: f64,
    brightness: f64,
    output_path: String,
) -> Result<AddonResult, String> {
    let bytes = vpkmerge_core::read_vpk_entry(&vpk_path, &entry).map_err(|e| format!("{e:#}"))?;
    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);
    let recolored =
        vpkmerge_core::recolor_texture_hue(&bytes, recolor).map_err(|e| format!("{e:#}"))?;
    vpkmerge_core::pack(&[(entry.as_str(), recolored.as_slice())], &output_path)
        .map_err(|e| format!("{e:#}"))?;
    Ok(AddonResult {
        output_path,
        entries: 1,
        summary: format!("Recolored {entry} (overrides the base texture in place)"),
    })
}

#[derive(Serialize)]
struct ModelColorBuffer {
    mesh_name: String,
    block_index: usize,
    vertex_count: usize,
}

/// List a model's color-bearing vertex buffers (the recolor candidates).
#[tauri::command]
async fn list_model_colors(
    vpk_path: String,
    entry: String,
    base_path: Option<String>,
) -> Result<Vec<ModelColorBuffer>, String> {
    let base = opt_path(base_path);
    let targets = vpkmerge_core::model_vertex_targets(&vpk_path, &entry, base.as_deref())
        .map_err(|e| format!("{e:#}"))?;
    Ok(targets
        .into_iter()
        .filter(|t| t.has_color)
        .map(|t| ModelColorBuffer {
            mesh_name: t.mesh_name,
            block_index: t.block_index,
            vertex_count: t.vertex_count,
        })
        .collect())
}

/// Recolor one model's baked per-vertex colors and pack it into an addon VPK.
#[tauri::command]
async fn recolor_model_to_addon(
    vpk_path: String,
    entry: String,
    base_path: Option<String>,
    hue: f64,
    saturation: f64,
    brightness: f64,
    output_path: String,
) -> Result<AddonResult, String> {
    let base = opt_path(base_path);
    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);
    let entries = vec![entry];
    let report =
        vpkmerge_core::recolor_models_to_addon(&vpk_path, &entries, base.as_deref(), recolor, &output_path)
            .map_err(|e| format!("{e:#}"))?;
    let total_verts: usize = report.iter().map(|r| r.stats.vertices).sum();
    Ok(AddonResult {
        output_path,
        entries: report.len(),
        summary: format!("{} model(s) recolored, {total_verts} vertices", report.len()),
    })
}

#[derive(Serialize)]
struct EventSummaryDto {
    name: String,
    base: Option<String>,
    vsnd_count: usize,
    volume: Option<f64>,
}

/// Load a `.vsndevts_c` (loose file or VPK entry) and return its event summaries.
#[tauri::command]
async fn load_soundevents(
    input: String,
    from_vpk: Option<String>,
) -> Result<Vec<EventSummaryDto>, String> {
    let from_vpk = from_vpk.filter(|s| !s.trim().is_empty());
    let se = match &from_vpk {
        Some(vpk) => vpkmerge_core::SoundEvents::from_vpk(vpk, &input),
        None => vpkmerge_core::SoundEvents::from_file(&input),
    }
    .map_err(|e| format!("{e:#}"))?;
    Ok(se
        .summaries()
        .into_iter()
        .map(|s| EventSummaryDto {
            name: s.name,
            base: s.base,
            vsnd_count: s.vsnd_count,
            volume: s.volume,
        })
        .collect())
}

#[derive(serde::Deserialize)]
struct SoundFieldEdit {
    event: String,
    field: String,
    value: f64,
}

#[derive(serde::Deserialize)]
struct SoundSwapEdit {
    from: String,
    to: String,
}

/// Apply the full edit batch (clip swaps + numeric field sets) to a
/// soundevents file, re-encode it uncompressed, and pack it into an addon VPK.
/// Stateless: the whole edit set arrives in one call.
#[tauri::command]
async fn build_soundevents_vpk(
    input: String,
    from_vpk: Option<String>,
    vpk_entry: Option<String>,
    sets: Vec<SoundFieldEdit>,
    swaps: Vec<SoundSwapEdit>,
    output_path: String,
) -> Result<AddonResult, String> {
    let from_vpk = from_vpk.filter(|s| !s.trim().is_empty());
    let mut se = match &from_vpk {
        Some(vpk) => vpkmerge_core::SoundEvents::from_vpk(vpk, &input),
        None => vpkmerge_core::SoundEvents::from_file(&input),
    }
    .map_err(|e| format!("{e:#}"))?;

    for swap in &swaps {
        se.swap_vsnd(&swap.from, &swap.to);
    }
    for set in &sets {
        if !se.set_event_field(&set.event, &set.field, set.value) {
            return Err(format!("no event named {:?}", set.event));
        }
    }

    let entry = match (vpk_entry.filter(|s| !s.trim().is_empty()), &from_vpk) {
        (Some(e), _) => e,
        (None, Some(_)) => input.clone(),
        (None, None) => {
            return Err("provide an entry path for a loose-file input".into());
        }
    };
    let bytes = se.encode().map_err(|e| format!("{e:#}"))?;
    vpkmerge_core::pack(&[(entry.as_str(), bytes.as_slice())], &output_path)
        .map_err(|e| format!("{e:#}"))?;
    Ok(AddonResult {
        output_path,
        entries: 1,
        summary: format!(
            "{entry}: {} field edit(s), {} swap(s)",
            sets.len(),
            swaps.len()
        ),
    })
}

#[tauri::command]
async fn save_text_file(
    app: AppHandle,
    default_name: String,
    content: String,
) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
async fn reveal_in_folder(path: String) -> Result<(), String> {
    use std::process::Command;
    let result = if cfg!(target_os = "linux") {
        let p = std::path::Path::new(&path);
        let target = if p.is_file() {
            p.parent().unwrap_or(p)
        } else {
            p
        };
        Command::new("xdg-open").arg(target).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").args(["-R", &path]).spawn()
    } else {
        return Err("Unsupported OS".into());
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            pick_vpk_files,
            pick_output_path,
            add_mod,
            detect_conflicts,
            merge_vpks,
            path_exists,
            reveal_in_folder,
            preview_texture,
            save_text_file,
            default_deadlock_vpk_path,
            default_addon_output_path,
            supported_hero_options,
            build_hero_prism_vpk,
            recolor_hero_vpk,
            recolor_hero_preview,
            scan_hero_rainbow,
            preview_texture_recolor,
            recolor_texture_to_addon,
            list_model_colors,
            recolor_model_to_addon,
            load_soundevents,
            build_soundevents_vpk
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
