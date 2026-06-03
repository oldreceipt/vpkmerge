//! Procedural "trippy" hero skin repainting.
//!
//! This promotes the Paradox prototype (`examples/reskin_chrono_trippy.rs`) into
//! a reusable core path: generate seamless psychedelic albedo textures, pack them
//! at the hero's existing texture paths, and byte-patch VMAT scroll vectors so the
//! paint flows at runtime. Discovery is conservative: only hero-namespaced model
//! materials/textures are touched, so shared engine textures are not overwritten.

use crate::hero_recolor::{
    animate_particle_timing_bytes, insert_color_cycle_operator_with_tuning,
    is_prism_animation_target, loop_animate_particle_bytes, recolor_material_color_bytes,
    GradientStop, PrismAnimationStyle, PrismGradient, PrismTuning,
};
use crate::{recipe_for, recolor_model_vertex_colors, recolor_particle_bytes, Recolor};
use anyhow::{Context, Result};
use morphic::kv3::{Seg, Value};
use morphic::{Image, ImageData, TextureFormat};
use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::TAU;
use std::path::Path;

pub const TRIPPY_STYLE_NAMES: &[&str] = &[
    "confetti", "liquid", "moire", "kaleido", "holo", "glitch", "thermal", "gradient",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrippyStyle {
    Confetti,
    Liquid,
    Moire,
    Kaleido,
    Holo,
    Glitch,
    Thermal,
    Gradient,
}

impl TrippyStyle {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confetti => "confetti",
            Self::Liquid => "liquid",
            Self::Moire => "moire",
            Self::Kaleido => "kaleido",
            Self::Holo => "holo",
            Self::Glitch => "glitch",
            Self::Thermal => "thermal",
            Self::Gradient => "gradient",
        }
    }

    pub fn from_name(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "confetti" | "acid" | "rainbow" => Ok(Self::Confetti),
            "liquid" | "marble" | "hololiquid" => Ok(Self::Liquid),
            "moire" | "opart" => Ok(Self::Moire),
            "kaleido" | "kaleidoscope" => Ok(Self::Kaleido),
            "holo" | "foil" | "holographic" => Ok(Self::Holo),
            "glitch" | "crt" => Ok(Self::Glitch),
            "thermal" | "xray" => Ok(Self::Thermal),
            "gradient" | "smooth" => Ok(Self::Gradient),
            other => anyhow::bail!(
                "unknown trippy style {other:?}; expected one of {}",
                TRIPPY_STYLE_NAMES.join(", ")
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrippySkinOptions {
    pub style: TrippyStyle,
    /// Texture blend, 0.0 = original texture, 1.0 = full generated pattern.
    pub intensity: f32,
    /// Hue/pattern phase offset, normalized 0..1.
    pub phase: f32,
    /// Runtime material scroll scale, 1.0 = Paradox prototype speed.
    pub scroll: f64,
    pub include_body: bool,
    pub include_weapons: bool,
}

impl Default for TrippySkinOptions {
    fn default() -> Self {
        Self {
            style: TrippyStyle::Confetti,
            intensity: 1.0,
            phase: 0.0,
            scroll: 1.0,
            include_body: true,
            include_weapons: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrippySkinReport {
    pub codename: String,
    pub body_textures: usize,
    pub weapon_textures: usize,
    pub materials_scrolled: usize,
    pub texture_placeholders_promoted: usize,
    pub skipped_shared_textures: usize,
    pub skipped_unreadable: usize,
    pub skipped_unpatchable_materials: usize,
    pub total_entries: usize,
}

#[derive(Debug, Clone)]
pub struct TrippyAbilityOptions {
    pub style: TrippyStyle,
    /// Texture blend strength; particle colors use this as saturation/value
    /// emphasis because particle color fields cannot be partially blended
    /// without a full particle-tree rewrite.
    pub intensity: f32,
    /// Hue/pattern phase offset, normalized 0..1.
    pub phase: f32,
    /// Strength of the optional particle animation pass.
    pub animation_intensity: f64,
    /// sweep = timing/scroll, loop = sweep + loop color gradients, cycle = loop +
    /// inserted runtime color-cycle operators where safe.
    pub animation_style: PrismAnimationStyle,
    pub include_abilities: bool,
    pub include_weapons: bool,
}

impl Default for TrippyAbilityOptions {
    fn default() -> Self {
        Self {
            style: TrippyStyle::Confetti,
            intensity: 1.0,
            phase: 0.0,
            animation_intensity: 1.0,
            animation_style: PrismAnimationStyle::Cycle,
            include_abilities: true,
            include_weapons: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrippyAbilityReport {
    pub codename: String,
    pub particles_total: usize,
    pub particles_recolored: usize,
    pub particles_animated: usize,
    pub particles_no_color: usize,
    pub particles_unpatchable: usize,
    pub texture_age_inputs: usize,
    pub texture_offset_multipliers: usize,
    pub gradient_timing_edits: usize,
    pub color_gradient_loops: usize,
    pub color_cycle_operators: usize,
    pub textures_painted: usize,
    pub textures_skipped: usize,
    pub materials_recolored: usize,
    pub materials_scrolled: usize,
    pub materials_unpatchable: usize,
    pub models_recolored: usize,
    pub model_vertices: usize,
    pub missing_entries: usize,
    pub total_entries: usize,
}

/// Render procedural trippy pattern frames for a live UI preview.
///
/// The returned PNGs are generated from the same pattern function used by the
/// skin/ability texture bake; advancing `phase` over the frame loop mirrors the
/// runtime UV/color flow without needing the game engine.
#[allow(clippy::cast_precision_loss)]
pub fn trippy_preview_frames(
    style: TrippyStyle,
    phase: f32,
    scroll: f32,
    intensity: f32,
    n_frames: usize,
    size: u32,
) -> Result<Vec<Vec<u8>>> {
    let n_frames = n_frames.clamp(1, 48);
    let size = size.clamp(16, 512);
    let mut out = Vec::with_capacity(n_frames);

    for frame in 0..n_frames {
        let t = frame as f32 / n_frames as f32;
        let frame_phase = phase + t * scroll;
        out.push(trippy_preview_png(style, frame_phase, intensity, size)?);
    }

    Ok(out)
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn trippy_preview_png(
    style: TrippyStyle,
    phase: f32,
    intensity: f32,
    size: u32,
) -> Result<Vec<u8>> {
    let mut pixels = vec![255; (size * size * 4) as usize];
    let blend = intensity.clamp(0.0, 1.0);
    for y in 0..size {
        let v = y as f32 / size.max(1) as f32;
        for x in 0..size {
            let u = x as f32 / size.max(1) as f32;
            let generated = trippy_pixel(style, u, v, phase);
            let shade = checker_shade(x, y);
            let i = ((y * size + x) * 4) as usize;
            for k in 0..3 {
                let base = shade;
                pixels[i + k] = (base + (f32::from(generated[k]) - base) * blend)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    let image = Image {
        width: size,
        height: size,
        data: ImageData::Rgba8(pixels),
    };
    morphic::encode_image(&image, TextureFormat::PngRgba8888).context("encoding trippy preview PNG")
}

#[allow(clippy::manual_is_multiple_of)]
fn checker_shade(x: u32, y: u32) -> f32 {
    if ((x / 16) + (y / 16)) % 2 == 0 {
        42.0
    } else {
        56.0
    }
}

#[derive(Debug, Clone)]
struct TrippyTarget {
    material_entry: String,
    texture_entry: String,
    weapon: bool,
    placeholder: bool,
}

/// Build a procedural trippy hero skin addon. Textures are discovered from
/// hero-namespaced VMATs under `models/heroes*`; weapon materials are included
/// when `options.include_weapons` is true.
#[allow(
    clippy::too_many_lines,
    clippy::single_match_else,
    clippy::manual_let_else
)]
pub fn trippy_skin_to_addon(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    codename: &str,
    options: &TrippySkinOptions,
    out: impl AsRef<Path>,
) -> Result<TrippySkinReport> {
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let targets = discover_targets(&vpks, codename, options)?;
    anyhow::ensure!(
        !targets.is_empty(),
        "no hero-specific body/weapon color textures found for {codename:?}"
    );

    let donor = find_donor_texture(&vpks, &targets)
        .context("no real hero color texture was available to use as an encode container")?;
    let mut packed: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut report = TrippySkinReport {
        codename: codename.to_string(),
        ..Default::default()
    };

    for target in &targets {
        let source = if target.placeholder {
            donor.as_slice()
        } else {
            match read_entry(&vpks, &target.texture_entry) {
                Some(bytes) => {
                    packed.insert(
                        target.texture_entry.clone(),
                        repaint_texture(&bytes, target, options)
                            .with_context(|| format!("painting {}", target.texture_entry))?,
                    );
                    if target.weapon {
                        report.weapon_textures += 1;
                    } else {
                        report.body_textures += 1;
                    }
                    continue;
                }
                None => {
                    report.skipped_unreadable += 1;
                    continue;
                }
            }
        };

        packed.insert(
            target.texture_entry.clone(),
            repaint_texture(source, target, options)
                .with_context(|| format!("painting {}", target.texture_entry))?,
        );
        if target.placeholder {
            report.texture_placeholders_promoted += 1;
        }
        if target.weapon {
            report.weapon_textures += 1;
        } else {
            report.body_textures += 1;
        }
    }

    for material in targets
        .iter()
        .map(|t| t.material_entry.as_str())
        .collect::<BTreeSet<_>>()
    {
        let Some(bytes) = read_entry(&vpks, material) else {
            report.skipped_unreadable += 1;
            continue;
        };
        let value = match morphic::decode_kv3_resource(&bytes) {
            Ok(value) => value,
            Err(_) => {
                report.skipped_unpatchable_materials += 1;
                continue;
            }
        };
        let weapon = is_weapon_path(material);
        let edits = trippy_material_edits(&value, options, weapon);
        if edits.is_empty() {
            continue;
        }
        match morphic::patch_kv3_resource_doubles(&bytes, &edits) {
            Ok(new_bytes) => {
                packed.insert(material.to_string(), new_bytes);
                report.materials_scrolled += 1;
            }
            Err(_) => report.skipped_unpatchable_materials += 1,
        }
    }

    let readme = format!(
        "Trippy hero skin\n\
         ================\n\
         Hero: {codename}\n\
         Style: {}\n\
         Intensity: {:.2}\n\
         Scroll: {:.2}\n\
         Targets: body={}, weapons={}\n\
         Generated by vpkmerge. Only hero-namespaced model materials/textures were touched;\n\
         shared engine textures were skipped.\n",
        options.style.as_str(),
        options.intensity,
        options.scroll,
        options.include_body,
        options.include_weapons,
    );
    packed.insert("README.txt".to_string(), readme.into_bytes());

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    crate::pack(&refs, out.as_ref())
        .with_context(|| format!("packing trippy skin into {}", out.as_ref().display()))?;
    report.total_entries = refs.len();
    Ok(report)
}

/// Build a trippy animated ability/weapon VFX addon for a pinned hero recipe.
///
/// This is the ability-side companion to [`trippy_skin_to_addon`]: color-bearing
/// particle params get a style-sampled hue, explicit ability textures receive the
/// same procedural paint patterns as skins, ability VMAT tint/scroll params are
/// patched when present, and high-visibility particles can receive runtime timing
/// / color-loop / color-cycle animation.
#[allow(clippy::too_many_lines)]
pub fn trippy_ability_vfx_to_addon(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    codename: &str,
    options: &TrippyAbilityOptions,
    out: impl AsRef<Path>,
) -> Result<TrippyAbilityReport> {
    let recipe = recipe_for(codename).with_context(|| {
        format!(
            "no built-in ability-VFX recipe for hero codename {codename:?} (pinned: {})",
            crate::pinned_hero_codenames().join(", ")
        )
    })?;
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut packed: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut missing = Vec::new();
    let mut report = TrippyAbilityReport {
        codename: recipe.codename.clone(),
        ..Default::default()
    };

    let particle_entries: Vec<String> = list_entries(&vpks, &recipe.particle_prefixes, ".vpcf_c")
        .into_iter()
        .filter(|entry| vfx_target_allowed(entry, options))
        .collect();
    report.particles_total = particle_entries.len();
    let cycle_tuning = trippy_cycle_tuning(options);

    for entry in &particle_entries {
        let bytes = read_entry(&vpks, entry)
            .with_context(|| format!("reading particle {entry} (listed but unreadable)"))?;
        let mut working = bytes;
        let mut changed = false;
        let mut had_color = false;

        if options.intensity > 0.0 {
            let recolor = trippy_recolor_for(&recipe.codename, entry, 0.0, options);
            match recolor_particle_bytes(&working, recolor) {
                Ok(Some(new_bytes)) => {
                    working = new_bytes;
                    changed = true;
                    had_color = true;
                    report.particles_recolored += 1;
                }
                Ok(None) => {}
                Err(e) => {
                    report.particles_unpatchable += 1;
                    eprintln!("  note: skipping particle color for {entry}: {e:#}");
                    continue;
                }
            }
        }

        let mut animated_this = false;
        if options.animation_intensity > 0.0 && is_prism_animation_target(entry) {
            match animate_particle_timing_bytes(&working, options.animation_intensity) {
                Ok(Some((new_bytes, stats))) => {
                    working = new_bytes;
                    changed = true;
                    animated_this = true;
                    report.texture_age_inputs += stats.age_inputs;
                    report.texture_offset_multipliers += stats.texture_offset_multipliers;
                    report.gradient_timing_edits += stats.gradient_timing_edits;
                }
                Ok(None) => {}
                Err(e) => eprintln!("  note: timing animation skipped for {entry}: {e:#}"),
            }

            if matches!(
                options.animation_style,
                PrismAnimationStyle::Loop | PrismAnimationStyle::Cycle
            ) {
                match loop_animate_particle_bytes(&working) {
                    Ok(Some(new_bytes)) => {
                        working = new_bytes;
                        changed = true;
                        animated_this = true;
                        report.color_gradient_loops += 1;
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("  note: color-gradient loop skipped for {entry}: {e:#}"),
                }
            }

            if options.animation_style == PrismAnimationStyle::Cycle {
                match insert_color_cycle_operator_with_tuning(&working, cycle_tuning) {
                    Ok(Some(new_bytes)) => {
                        working = new_bytes;
                        changed = true;
                        animated_this = true;
                        report.color_cycle_operators += 1;
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("  note: color-cycle insert skipped for {entry}: {e:#}"),
                }
            }
        }

        if animated_this {
            report.particles_animated += 1;
        }
        if changed {
            packed.insert(entry.clone(), working);
        } else if !had_color {
            report.particles_no_color += 1;
        }
    }

    for entry in &recipe.texture_entries {
        if !vfx_target_allowed(entry, options) {
            continue;
        }
        match read_entry(&vpks, entry) {
            Some(bytes) => match repaint_ability_texture(&bytes, options) {
                Ok(new_bytes) => {
                    packed.insert(entry.clone(), new_bytes);
                    report.textures_painted += 1;
                }
                Err(e) => {
                    report.textures_skipped += 1;
                    eprintln!("  note: texture paint skipped for {entry}: {e:#}");
                }
            },
            None => missing.push(entry.clone()),
        }
    }

    for entry in &recipe.material_entries {
        if !vfx_target_allowed(entry, options) {
            continue;
        }
        let Some(bytes) = read_entry(&vpks, entry) else {
            missing.push(entry.clone());
            continue;
        };
        let mut working = bytes;
        let mut changed = false;

        if options.intensity > 0.0 {
            let recolor = trippy_recolor_for(&recipe.codename, entry, 0.33, options);
            match recolor_material_color_bytes(&working, recolor) {
                Ok(Some(new_bytes)) => {
                    working = new_bytes;
                    changed = true;
                    report.materials_recolored += 1;
                }
                Ok(None) => {}
                Err(e) => {
                    report.materials_unpatchable += 1;
                    eprintln!("  note: material tint skipped for {entry}: {e:#}");
                }
            }
        }

        if options.animation_intensity > 0.0 {
            match morphic::decode_kv3_resource(&working) {
                Ok(value) => {
                    let edits = trippy_material_edits(
                        &value,
                        &skin_options_from_ability(options),
                        is_weapon_path(entry),
                    );
                    if !edits.is_empty() {
                        match morphic::patch_kv3_resource_doubles(&working, &edits) {
                            Ok(new_bytes) => {
                                working = new_bytes;
                                changed = true;
                                report.materials_scrolled += 1;
                            }
                            Err(e) => {
                                report.materials_unpatchable += 1;
                                eprintln!("  note: material scroll skipped for {entry}: {e:#}");
                            }
                        }
                    }
                }
                Err(e) => eprintln!("  note: material scroll decode skipped for {entry}: {e:#}"),
            }
        }

        if changed {
            packed.insert(entry.clone(), working);
        }
    }

    for entry in &recipe.model_entries {
        if !vfx_target_allowed(entry, options) || options.intensity <= 0.0 {
            continue;
        }
        match read_entry(&vpks, entry) {
            Some(bytes) => {
                let recolor = trippy_recolor_for(&recipe.codename, entry, 0.66, options);
                let (new_bytes, stats) = recolor_model_vertex_colors(&bytes, recolor)
                    .with_context(|| format!("trippy-recoloring model {entry}"))?;
                if stats.buffers_recolored == 0 {
                    continue;
                }
                packed.insert(entry.clone(), new_bytes);
                report.models_recolored += 1;
                report.model_vertices += stats.vertices;
            }
            None => missing.push(entry.clone()),
        }
    }

    report.missing_entries = missing.len();
    if !missing.is_empty() {
        anyhow::bail!(
            "{} recipe entr{} not found in the given VPK(s) (recipe drift?): {}",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" },
            missing.join(", ")
        );
    }

    let readme = format!(
        "Trippy ability VFX\n\
         ==================\n\
         Hero: {}\n\
         Style: {}\n\
         Intensity: {:.2}\n\
         Animation: {:?} @ {:.2}\n\
         Targets: abilities={}, weapons={}\n\
         Generated by vpkmerge. Ability/weapon particle entries come from the pinned hero VFX recipe;\n\
         explicit ability textures are painted with procedural trippy patterns.\n",
        report.codename,
        options.style.as_str(),
        options.intensity,
        options.animation_style,
        options.animation_intensity,
        options.include_abilities,
        options.include_weapons,
    );
    packed.insert("README.txt".to_string(), readme.into_bytes());

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    crate::pack(&refs, out.as_ref())
        .with_context(|| format!("packing trippy ability VFX into {}", out.as_ref().display()))?;
    report.total_entries = refs.len();
    Ok(report)
}

#[allow(clippy::unnecessary_wraps)]
fn discover_targets(
    vpks: &[valve_pak::VPK],
    codename: &str,
    options: &TrippySkinOptions,
) -> Result<Vec<TrippyTarget>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut materials = BTreeSet::new();
    for vpk in vpks {
        for path in vpk.file_paths() {
            if path.ends_with(".vmat_c")
                && path.starts_with("models/heroes")
                && hero_path_match(path, codename)
            {
                materials.insert(path.clone());
            }
        }
    }

    for material_entry in materials {
        let weapon = is_weapon_path(&material_entry);
        if (weapon && !options.include_weapons) || (!weapon && !options.include_body) {
            continue;
        }
        let Some(bytes) = read_entry(vpks, &material_entry) else {
            continue;
        };
        let Ok(mat) = morphic::material::parse(&bytes) else {
            continue;
        };
        let Some(texture) = base_color_texture(&mat) else {
            continue;
        };
        let texture_entry = compiled_resource_path(texture);
        if !hero_path_match(&texture_entry, codename) || !texture_entry.starts_with("models/heroes")
        {
            continue;
        }
        let placeholder = read_entry(vpks, &texture_entry)
            .and_then(|b| morphic::inspect(&b).ok())
            .is_some_and(|info| u32::from(info.width) <= 8 || u32::from(info.height) <= 8);
        if seen.insert((material_entry.clone(), texture_entry.clone())) {
            out.push(TrippyTarget {
                material_entry,
                texture_entry,
                weapon,
                placeholder,
            });
        }
    }
    Ok(out)
}

fn base_color_texture(mat: &morphic::material::Material) -> Option<&str> {
    mat.texture("g_tColor")
        .or_else(|| mat.texture("g_tColorA"))
        .or_else(|| mat.texture("g_tBaseColor"))
        .or_else(|| mat.texture("g_tAlbedo"))
}

fn find_donor_texture(vpks: &[valve_pak::VPK], targets: &[TrippyTarget]) -> Option<Vec<u8>> {
    for target in targets {
        if target.placeholder {
            continue;
        }
        let Some(bytes) = read_entry(vpks, &target.texture_entry) else {
            continue;
        };
        let Ok(info) = morphic::inspect(&bytes) else {
            continue;
        };
        if u32::from(info.width) > 8 && u32::from(info.height) > 8 {
            return Some(bytes);
        }
    }
    None
}

fn repaint_texture(
    bytes: &[u8],
    target: &TrippyTarget,
    options: &TrippySkinOptions,
) -> Result<Vec<u8>> {
    let mut image = morphic::decode(bytes).context("decoding trippy texture")?;
    let style = if target.weapon && target.placeholder {
        TrippyStyle::Gradient
    } else {
        options.style
    };
    paint_image(&mut image, style, options.phase, options.intensity)?;
    morphic::replace_mip_chain(bytes, &image).context("re-encoding trippy texture")
}

fn repaint_ability_texture(bytes: &[u8], options: &TrippyAbilityOptions) -> Result<Vec<u8>> {
    let mut image = morphic::decode(bytes).context("decoding trippy ability texture")?;
    paint_image(&mut image, options.style, options.phase, options.intensity)?;
    morphic::replace_mip_chain(bytes, &image).context("re-encoding trippy ability texture")
}

fn skin_options_from_ability(options: &TrippyAbilityOptions) -> TrippySkinOptions {
    TrippySkinOptions {
        style: options.style,
        intensity: options.intensity,
        phase: options.phase,
        scroll: options.animation_intensity,
        include_body: true,
        include_weapons: true,
    }
}

fn vfx_target_allowed(entry: &str, options: &TrippyAbilityOptions) -> bool {
    let weapon = entry.contains("/weapon_fx/") || is_weapon_path(entry);
    if weapon {
        options.include_weapons
    } else {
        options.include_abilities
    }
}

fn trippy_cycle_tuning(options: &TrippyAbilityOptions) -> PrismTuning {
    let gradient = match options.style {
        TrippyStyle::Thermal => PrismGradient::preset("fire"),
        TrippyStyle::Liquid => PrismGradient::preset("ocean"),
        TrippyStyle::Moire | TrippyStyle::Glitch => PrismGradient::preset("neon"),
        TrippyStyle::Kaleido => PrismGradient::preset("sunset"),
        TrippyStyle::Holo => PrismGradient::from_stops(&[
            GradientStop {
                position: 0.0,
                hue: 190.0,
                saturation: 0.45,
            },
            GradientStop {
                position: 0.48,
                hue: 305.0,
                saturation: 0.62,
            },
            GradientStop {
                position: 1.0,
                hue: 55.0,
                saturation: 0.38,
            },
        ]),
        TrippyStyle::Confetti | TrippyStyle::Gradient => None,
    };
    let intensity = f64::from(options.intensity.max(0.0));
    PrismTuning {
        hue_offset: f64::from(options.phase) * 360.0,
        saturation: (0.85 + 0.25 * intensity).clamp(0.25, 1.5),
        brightness: (0.90 + 0.18 * intensity).clamp(0.25, 1.6),
        animation_intensity: options.animation_intensity,
        animation_style: options.animation_style,
        gradient,
    }
}

fn trippy_recolor_for(
    codename: &str,
    entry: &str,
    offset: f32,
    options: &TrippyAbilityOptions,
) -> Recolor {
    let a = hash_str01(&format!("{codename}:{entry}:a"));
    let b = hash_str01(&format!("{entry}:{codename}:b"));
    let rgb = trippy_pixel(
        options.style,
        (a + offset).fract(),
        (b + offset * 0.618).fract(),
        options.phase,
    );
    let (hue, sat, val) = rgb_to_hsv(rgb);
    let intensity = f64::from(options.intensity.max(0.0));
    Recolor::new(
        hue,
        (0.72 + sat * 0.48 * intensity).clamp(0.25, 2.0),
        (0.82 + (val - 0.5) * 0.55 * intensity).clamp(0.25, 1.8),
    )
}

fn trippy_material_edits(
    value: &Value,
    options: &TrippySkinOptions,
    weapon: bool,
) -> Vec<(Vec<Seg>, f64)> {
    let scroll = [0.08 * options.scroll, 0.05 * options.scroll];
    let mut edits = Vec::new();
    edits.extend(scroll_xy_edits(value, "g_vAlbedoScrollSpeed1", scroll));
    edits.extend(scroll_xy_edits(value, "g_vSelfIllumScrollSpeed1", scroll));
    if matches!(options.style, TrippyStyle::Holo) {
        edits.extend(scroll_xy_edits(
            value,
            "g_vNormalAndRoughnessScrollSpeed1",
            [0.12 * options.scroll, 0.07 * options.scroll],
        ));
        edits.extend(float_param_edit(
            value,
            "g_flSelfIllumScale1",
            5.5 * f64::from(options.intensity.max(0.0)),
        ));
        edits.extend(float_param_edit(
            value,
            "g_flSelfIllumFresnelMaskExponent",
            2.5,
        ));
        edits.extend(vcomp_edits(
            value,
            "g_vSelfIllumFresnelMaskTint1",
            &[(0, 1.0), (1, 1.0), (2, 1.0)],
        ));
    }
    if weapon {
        edits.extend(vcomp_edits(
            value,
            "TextureColor1",
            &[(0, 1.0), (1, 1.0), (2, 1.0)],
        ));
    }
    edits
}

#[allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn paint_image(image: &mut Image, style: TrippyStyle, phase: f32, intensity: f32) -> Result<()> {
    let (w, h) = (image.width, image.height);
    let ImageData::Rgba8(px) = &mut image.data else {
        anyhow::bail!("trippy skin supports LDR (8-bit) textures only");
    };
    let blend = intensity.clamp(0.0, 1.0);
    for y in 0..h {
        let v = y as f32 / h.max(1) as f32;
        for x in 0..w {
            let u = x as f32 / w.max(1) as f32;
            let generated = trippy_pixel(style, u, v, phase);
            let i = ((y * w + x) * 4) as usize;
            for k in 0..3 {
                let original = f32::from(px[i + k]);
                px[i + k] = (original + (f32::from(generated[k]) - original) * blend)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

fn trippy_pixel(style: TrippyStyle, u: f32, v: f32, phase: f32) -> [u8; 3] {
    match style {
        TrippyStyle::Confetti => confetti(u, v, phase),
        TrippyStyle::Liquid => liquid(u, v, phase),
        TrippyStyle::Moire => moire(u, v, phase),
        TrippyStyle::Kaleido => kaleido(u, v, phase),
        TrippyStyle::Holo => holo(u, v, phase),
        TrippyStyle::Glitch => glitch(u, v, phase),
        TrippyStyle::Thermal => thermal(u, v, phase),
        TrippyStyle::Gradient => gradient(u, v, phase),
    }
}

#[allow(clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn hash2(i: i64, j: i64) -> f32 {
    let mut h = (i
        .wrapping_mul(374_761_393)
        .wrapping_add(j.wrapping_mul(668_265_263))) as u64;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xff_ffff) as f32 / 16_777_216.0
}

#[allow(clippy::cast_precision_loss)]
fn hash_str01(s: &str) -> f32 {
    let mut h = 14_695_981_039_346_656_037_u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1_099_511_628_211);
    }
    ((h >> 40) & 0xff_ffff) as f32 / 16_777_216.0
}

#[allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
fn vnoise(x: f32, y: f32, p: i64) -> f32 {
    let gx = x * p as f32;
    let gy = y * p as f32;
    let x0 = gx.floor() as i64;
    let y0 = gy.floor() as i64;
    let fx = gx - x0 as f32;
    let fy = gy - y0 as f32;
    let wrap = |a: i64| ((a % p) + p) % p;
    let s = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let ux = s(fx);
    let uy = s(fy);
    let v00 = hash2(wrap(x0), wrap(y0));
    let v10 = hash2(wrap(x0 + 1), wrap(y0));
    let v01 = hash2(wrap(x0), wrap(y0 + 1));
    let v11 = hash2(wrap(x0 + 1), wrap(y0 + 1));
    let a = v00 + (v10 - v00) * ux;
    let b = v01 + (v11 - v01) * ux;
    a + (b - a) * uy
}

fn fbm(x: f32, y: f32, p0: i64, oct: u32) -> f32 {
    let (mut sum, mut amp, mut p, mut norm) = (0.0, 0.5, p0, 0.0);
    for _ in 0..oct {
        sum += amp * vnoise(x, y, p);
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

#[allow(clippy::many_single_char_names, clippy::cast_possible_truncation)]
fn hsv2rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

fn rgb_to_hsv(rgb: [u8; 3]) -> (f64, f64, f64) {
    let r = f64::from(rgb[0]) / 255.0;
    let g = f64::from(rgb[1]) / 255.0;
    let b = f64::from(rgb[2]) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let hue = if d <= f64::EPSILON {
        0.0
    } else if (max - r).abs() <= f64::EPSILON {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() <= f64::EPSILON {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let sat = if max <= f64::EPSILON { 0.0 } else { d / max };
    (hue, sat, max)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pack_rgb(rgb: [f32; 3]) -> [u8; 3] {
    [
        (rgb[0] * 255.0).clamp(0.0, 255.0) as u8,
        (rgb[1] * 255.0).clamp(0.0, 255.0) as u8,
        (rgb[2] * 255.0).clamp(0.0, 255.0) as u8,
    ]
}

fn confetti(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let wu = u + 0.12 * fbm(u, v, 6, 4);
    let wv = v + 0.12 * fbm(u + 5.2, v + 1.3, 6, 4);
    let base = fbm(wu, wv, 5, 5);
    let mid = fbm(wu + 2.7, wv + 8.1, 18, 4);
    let fine = fbm(wu + 9.3, wv + 3.4, 40, 3);
    let huef = (base + mid * 1.6 + fine * 2.2).fract();
    let bands = 14.0_f32;
    let hue_q = (huef * bands).floor() / bands + 0.5 / bands;
    let hue = hue_q * 360.0 + phase * 360.0;
    let sat = (0.82 + 0.18 * fine).clamp(0.0, 1.0);
    let val = (0.55 + 0.42 * fbm(wu + 1.1, wv + 6.6, 24, 3)).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

fn liquid(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.5;
    let q0 = fbm(u, v, 4, 5);
    let q1 = fbm(u + 3.1, v + 6.2, 4, 5);
    let r0 = fbm(u + warp * q0 + 1.7, v + warp * q1 + 9.2, 5, 5);
    let r1 = fbm(u + warp * q0 + 8.3, v + warp * q1 + 2.8, 5, 5);
    let f = fbm(u + warp * r0, v + warp * r1, 6, 5);
    let veins = ((f * 11.0 + r0 * 3.0) * TAU).sin() * 0.5 + 0.5;
    let hue = (f * 2.4 + r0 * 0.9 + phase).fract() * 360.0;
    let sat = (0.80 + 0.20 * q1).clamp(0.0, 1.0);
    let val = (0.30 + 0.68 * veins).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

fn moire(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let g = |a: f32, b: f32, ph: f32| (TAU * (a * u + b * v) + ph).sin();
    let p = phase * TAU;
    let m1 = g(9.0, 4.0, p) + g(10.0, 5.0, -p);
    let m2 = g(4.0, 9.0, p * 1.3) + g(5.0, 8.0, -p);
    let field = (m1 * m2) * 0.25 + 0.5;
    let hue = (field * 1.6 + 0.12 * m1 + phase).fract() * 360.0;
    let val = (0.42 + 0.5 * ((field * 6.0).sin() * 0.5 + 0.5)).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, 0.95, val))
}

fn kaleido(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let g = |a: f32, b: f32| (TAU * (a * u + b * v)).cos();
    let k = 6.0;
    let mandala = (g(k, 0.0) + g(0.0, k) + 0.7 * g(k, k) + 0.7 * g(k, -k)) * 0.25;
    let radial = ((TAU * u).cos() + (TAU * v).cos()) * 0.5;
    let field = mandala * 0.5 + 0.5;
    let hue = (field + 0.35 * radial + phase).fract() * 360.0;
    let sat = (0.85 + 0.15 * radial).clamp(0.0, 1.0);
    let val = (0.45 + 0.5 * (mandala * 0.5 + 0.5)).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

fn holo(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.06 * fbm(u + 1.3, v + 4.1, 6, 3);
    let sweep = (u * 2.0 + v + warp).fract();
    let foil = ((u * 48.0 - v * 48.0) * TAU).sin() * 0.5 + 0.5;
    let hue = (sweep + 0.04 * foil + phase).fract() * 360.0;
    let sat = (0.45 + 0.30 * foil).clamp(0.0, 1.0);
    let val = (0.72 + 0.28 * foil).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

#[allow(clippy::cast_possible_truncation)]
fn glitch(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let band = (v * 28.0).floor() as i64;
    let r = hash2(band, 7);
    let shift = if r > 0.62 { (r - 0.62) * 0.9 } else { 0.0 };
    let warp = 0.15 * fbm(u, v, 10, 3);
    let hue_at = |off: f32| (((u + shift + warp + off) * 2.0 + phase).fract()) * 360.0;
    let rr = hsv2rgb(hue_at(0.0), 0.92, 0.9)[0];
    let gg = hsv2rgb(hue_at(0.010), 0.92, 0.9)[1];
    let bb = hsv2rgb(hue_at(0.020), 0.92, 0.9)[2];
    let scan = 0.55 + 0.45 * ((v * 512.0 * TAU).sin().abs());
    pack_rgb([rr * scan, gg * scan, bb * scan])
}

fn thermal_color(t: f32) -> [f32; 3] {
    const STOPS: &[(f32, [f32; 3])] = &[
        (0.00, [0.0, 0.0, 0.05]),
        (0.22, [0.20, 0.0, 0.45]),
        (0.42, [0.65, 0.0, 0.55]),
        (0.60, [0.95, 0.10, 0.10]),
        (0.76, [1.0, 0.55, 0.0]),
        (0.90, [1.0, 0.95, 0.25]),
        (1.00, [1.0, 1.0, 1.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t <= b {
            let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
            return [
                ca[0] + (cb[0] - ca[0]) * k,
                ca[1] + (cb[1] - ca[1]) * k,
                ca[2] + (cb[2] - ca[2]) * k,
            ];
        }
    }
    [1.0, 1.0, 1.0]
}

fn thermal(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.25 * fbm(u + 1.7, v + 4.2, 6, 4);
    let heat =
        (fbm(u + warp, v + warp, 6, 5) * 1.5 + 0.25 * fbm(u + 9.0, v + 2.0, 24, 3) + phase).fract();
    pack_rgb(thermal_color(heat))
}

fn gradient(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let hue = ((u * 2.0 + v).fract() + phase).fract() * 360.0;
    pack_rgb(hsv2rgb(hue, 0.95, 0.92))
}

fn vector_param_index(v: &Value, name: &str) -> Option<usize> {
    v.get("m_vectorParams")?
        .as_array()?
        .iter()
        .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
}

fn float_param_edit(v: &Value, name: &str, val: f64) -> Vec<(Vec<Seg>, f64)> {
    let Some(i) = v
        .get("m_floatParams")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
        })
    else {
        return Vec::new();
    };
    vec![(
        vec![
            Seg::Key("m_floatParams".to_string()),
            Seg::Index(i),
            Seg::Key("m_flValue".to_string()),
        ],
        val,
    )]
}

fn vcomp_edits(v: &Value, name: &str, comps: &[(usize, f64)]) -> Vec<(Vec<Seg>, f64)> {
    let Some(i) = vector_param_index(v, name) else {
        return Vec::new();
    };
    comps
        .iter()
        .map(|&(k, val)| {
            (
                vec![
                    Seg::Key("m_vectorParams".to_string()),
                    Seg::Index(i),
                    Seg::Key("m_value".to_string()),
                    Seg::Index(k),
                ],
                val,
            )
        })
        .collect()
}

fn scroll_xy_edits(v: &Value, name: &str, xy: [f64; 2]) -> Vec<(Vec<Seg>, f64)> {
    vcomp_edits(v, name, &[(0, xy[0]), (1, xy[1])])
}

fn compiled_resource_path(path: &str) -> String {
    if path.ends_with("_c") {
        path.to_string()
    } else {
        format!("{path}_c")
    }
}

fn hero_path_match(path: &str, codename: &str) -> bool {
    let p = path.to_ascii_lowercase();
    let c = codename.to_ascii_lowercase();
    p.contains(&format!("/{c}/"))
        || p.contains(&format!("/{c}_"))
        || p.contains(&format!("_{c}_"))
        || p.contains(&format!("/{c}."))
}

fn is_weapon_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [
        "weapon", "gun", "rifle", "pistol", "shotgun", "cannon", "launcher", "bow", "arrow",
        "blade", "sword", "knife", "staff", "wand", "mace", "club",
    ]
    .iter()
    .any(|needle| p.contains(needle))
}

fn open_vpks(vpk: &Path, base: Option<&Path>) -> Result<Vec<valve_pak::VPK>> {
    let mut vpks =
        vec![valve_pak::open(vpk).with_context(|| format!("opening {}", vpk.display()))?];
    if let Some(base) = base {
        vpks.push(valve_pak::open(base).with_context(|| format!("opening {}", base.display()))?);
    }
    Ok(vpks)
}

fn read_entry(vpks: &[valve_pak::VPK], entry: &str) -> Option<Vec<u8>> {
    for vpk in vpks {
        if let Ok(mut vf) = vpk.get_file(entry) {
            if let Ok(bytes) = vf.read_all() {
                return Some(bytes);
            }
        }
    }
    None
}

fn list_entries(vpks: &[valve_pak::VPK], prefixes: &[String], suffix: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for vpk in vpks {
        for p in vpk.file_paths() {
            if p.ends_with(suffix) && prefixes.iter().any(|prefix| p.starts_with(prefix)) {
                seen.insert(p.clone());
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_aliases_parse() {
        assert_eq!(TrippyStyle::from_name("crt").unwrap(), TrippyStyle::Glitch);
        assert_eq!(
            TrippyStyle::from_name("holographic").unwrap(),
            TrippyStyle::Holo
        );
        assert!(TrippyStyle::from_name("plain").is_err());
    }

    #[test]
    fn weapon_paths_are_detected() {
        assert!(is_weapon_path(
            "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun.vmat_c"
        ));
        assert!(!is_weapon_path(
            "models/heroes_staging/chrono/materials/chrono_v2.vmat_c"
        ));
    }

    #[test]
    fn preview_frames_are_pngs() {
        let frames =
            trippy_preview_frames(TrippyStyle::Holo, 0.0, 1.0, 1.0, 3, 32).expect("preview");
        assert_eq!(frames.len(), 3);
        for frame in frames {
            assert!(frame.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }
}
