//! Compose a hero's full ability-VFX recolor into one addon VPK.
//!
//! A Deadlock hero's ability effects carry color through three independent
//! mechanisms, each needing a different edit (see
//! `../grimoire/docs/ability-vfx-recolor.md`):
//!
//! 1. **Particle params** (`.vpcf_c`): `m_ConstantColor` / gradient stops, edited
//!    by an in-place KV3 scalar patch ([`morphic::patch_kv3_resource_scalars`]).
//! 2. **Color-bearing textures** (`.vtex_c`): self-illum / albedo color maps that
//!    a particle param can only multiply over, so they get their own hue shift
//!    ([`crate::recolor::recolor_texture_hue`]).
//! 3. **Mesh vertex colors** (`.vmdl_c`): baked per-vertex `COLOR` (Paige's ult
//!    horse/knight), reachable only by editing the mesh
//!    ([`crate::recolor::recolor_model_vertex_colors`]).
//!
//! All three use the **same absolute `set_color`** (set hue, scale each source
//! color's saturation + value), so one [`crate::Recolor`] target lands particles,
//! textures, and models on a single color. This module reads the recipe's entries out of a VPK (a skin
//! VPK first, then the base pak), recolors each, and packs the whole set into one
//! standalone addon that overrides the base in place: the single-call bridge a
//! mod manager drives.
//!
//! The recipe is currently a built-in per-hero table (Paige / `bookworm` only),
//! pinned from the in-game-verified recolor work. Generalizing it to automatic
//! discovery is a later step; the composition here does not change when it does.

use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

use morphic::kv3::{Seg, Value};

use crate::recolor::{set_color, Recolor};

/// The fixed set of base-game entries that carry one hero's ability-VFX color,
/// grouped by the recolor mechanism each needs.
#[derive(Debug, Clone)]
pub struct HeroRecolorRecipe {
    /// Model/particle codename (Paige = `bookworm`), the namespace `models/` and
    /// `particles/abilities/` use (NOT the sound codename).
    pub codename: String,
    /// `particles/{abilities,weapon_fx}/<codename>/` roots; every `.vpcf_c` under
    /// these gets its color params retinted.
    pub particle_prefixes: Vec<String>,
    /// Color-bearing `.vtex_c` entries (self-illum / albedo color maps).
    pub texture_entries: Vec<String>,
    /// `.vmat_c` entries whose ability color is a material `g_vColorTint` constant
    /// (not a texture): the `g_t*color` slot is a flat 4x4 placeholder, so the color
    /// is retinted by an in-place double patch ([`recolor_material_color_bytes`]).
    pub material_entries: Vec<String>,
    /// `.vmdl_c` entries whose color is baked into mesh vertex colors.
    pub model_entries: Vec<String>,
    /// A representative color-bearing `.vtex_c` (one of `texture_entries`) to
    /// render as a fast, no-re-encode UI preview swatch (see
    /// [`recolor_hero_preview_png`]). Pick one that reads as the hero's main
    /// ability glow, not a tiny swatch or a near-black map. `None` for a
    /// particle-only hero (no color texture to swatch); the preview path then
    /// errors rather than guessing one.
    pub preview_texture: Option<String>,
}

/// The built-in recipe for a hero codename, or `None` if no recipe is pinned yet.
#[must_use]
pub fn recipe_for(codename: &str) -> Option<HeroRecolorRecipe> {
    let codename = codename.to_lowercase();
    match codename.as_str() {
        "bookworm" => Some(paige_recipe()),
        // Particle-only heroes: ability/weapon VFX carry color purely through
        // `.vpcf_c` color params (no color textures or baked vertex colors), so one
        // `--hue` lands the whole ability/weapon set and nothing on the skin. Each
        // was pinned from an in-game recolor mod and confirmed with
        // `examples/particle_scan.rs` (base vs mod). Source hue is noted for
        // reference; the actual target hue is supplied at recolor time.
        //   codename    hero       src pak  .vpcf_c edited  source hue
        //   unicorn     Celeste    pak07    228             ~329 (pink)
        //   gigawatt    Seven      pak01    170             ~300 (magenta)
        //   vampirebat  Mina       pak05    117             ~295 (pink)
        //   necro       Graves     pak09    246             ~330 (pink)
        //   wraith      Wraith     pak11    127             ~330 (pink)
        //   inferno     Infernus   pak08    143             ~220 (blue)
        // Seven/Wraith/Infernus carry KV3 v4 particles; the in-place patcher handles
        // v4 + v5, so all recolor at full coverage.
        //
        // Graves (`necro`) is NOT particle-only (see `necro_recipe`): her large ability
        // maps are grayscale (particle-tinted), but the gravestone's transmissive glow
        // is a small chromatic texture, and the pickup-sphere / ult-jar color is a
        // material `g_vColorTint` CONSTANT (its `g_tColor` slot is a flat 4x4
        // placeholder), retinted by the in-place double patch. So her recipe adds one
        // texture + two materials on top of the particles.
        "necro" => Some(necro_recipe()),
        // Infernus (`inferno`): particles + the fire textures the flames sample. The
        // reference blue recolor does NOT touch `inferno_body.vmat_c` (the body tint
        // is not what colors his fire); it recolors the particles + the fire ramp /
        // burning / lava textures. We match that: no body material, recolor the
        // vanilla fire textures in place. See [`inferno_recipe`].
        "inferno" => Some(inferno_recipe()),
        "yamato" => Some(yamato_recipe()),
        "unicorn" | "gigawatt" | "vampirebat" | "wraith" => Some(particle_only_recipe(&codename)),
        _ => None,
    }
}

/// Paige (`bookworm`). Pinned from the in-game-verified purple recolor:
/// `pak02` (particles), `pak04` (the 9 color textures), and the ult vertex-color
/// addon (`models/particle/bookworm_horse_knight` + `bookworm_mace`). Source of
/// truth: `../grimoire/docs/ability-vfx-recolor.md` + `docs/handoff-vertex-color-recolor.md`.
fn paige_recipe() -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: "bookworm".to_string(),
        particle_prefixes: vec![
            "particles/abilities/bookworm/".to_string(),
            "particles/weapon_fx/bookworm/".to_string(),
        ],
        texture_entries: [
            // bullets (projectile self-illum)
            "materials/particle/abilities/bookworm/bookworm_projectile_self_illum_vmat_g_tcolor_7b26a19f.vtex_c",
            // AOE ground (hero-named in a shared dir)
            "materials/particle/projected/bookworm_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c",
            // ground streak
            "materials/particle/ground/ground_streak_bookworm_psd_5a44028c.vtex_c",
            // model self-illum / albedo color maps
            "models/heroes_wip/bookworm/materials/bookworm_ui_effects_color_psd_a29be817.vtex_c",
            "models/heroes_wip/bookworm/materials/bookworm_shield_illustrated_color_psd_81f5497b.vtex_c",
            "models/heroes_wip/bookworm/materials/bookworm_sword_illustrated_color_psd_4eb22603.vtex_c",
            "models/heroes_wip/bookworm/materials/bookworm_stone_illustrated_color_psd_8ed29960.vtex_c",
            "models/heroes_wip/bookworm/materials/bookworm_dragon_color_tga_ed3d3b5.vtex_c",
            "materials/models/particle/bookworm/neutral_black_dragon_color_psd_b8c8249f.vtex_c",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect(),
        // Paige's ability color is all real textures + vertex colors, no tint
        // constants.
        material_entries: Vec::new(),
        model_entries: [
            // the ult body that actually renders (found via the ult model particle)
            "models/particle/bookworm_horse_knight.vmdl_c",
            // melee swing
            "models/particle/bookworm_mace.vmdl_c",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect(),
        // The general ability effects color map: a large, clearly-chromatic
        // texture that reads as Paige's glow, so the UI swatch is representative.
        preview_texture: Some(
            "models/heroes_wip/bookworm/materials/bookworm_ui_effects_color_psd_a29be817.vtex_c"
                .to_string(),
        ),
    }
}

/// Particle-only recipe: just the two `particles/{abilities,weapon_fx}/<codename>/`
/// prefixes, no color textures or vertex-color models. The shape shared by every
/// hero whose ability VFX carry color purely through `.vpcf_c` color params, so one
/// `--hue` lands the whole ability/weapon set and touches nothing on the skin. The
/// per-hero provenance (source mod + scanned hue) is tabulated at [`recipe_for`].
fn particle_only_recipe(codename: &str) -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: codename.to_string(),
        particle_prefixes: vec![
            format!("particles/abilities/{codename}/"),
            format!("particles/weapon_fx/{codename}/"),
        ],
        texture_entries: Vec::new(),
        material_entries: Vec::new(),
        model_entries: Vec::new(),
        // Particle-only: no color texture to render as a swatch.
        preview_texture: None,
    }
}

/// Infernus (`inferno`): recolor his fire via the particle color params. This
/// matches the reference blue recolor, which does NOT touch `inferno_body.vmat_c`
/// (the body tint is not what colors his fire).
///
/// The reference also recolors the fire ramp / burning / lava textures, but it
/// does so on hero-specific COPIES it repoints the particles to. The vanilla fire
/// textures are SHARED across every fire effect in the game, so recoloring them in
/// place would tint everyone's fire, not just Infernus's. Hero-isolated fire-texture
/// recolor needs a rename+repoint step (recolor to a new path, edit the particle's
/// texture reference) that is not built yet, so for now Infernus is recolored by
/// particle params alone: his fire reads the picked hue but keeps the vanilla fire
/// texture's luminance/shape.
fn inferno_recipe() -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: "inferno".to_string(),
        particle_prefixes: vec![
            "particles/abilities/inferno/".to_string(),
            "particles/weapon_fx/inferno/".to_string(),
        ],
        texture_entries: Vec::new(),
        material_entries: Vec::new(),
        model_entries: Vec::new(),
        // No representative color texture, so no preview swatch.
        preview_texture: None,
    }
}

/// Yamato: most ability/weapon VFX color lives in particle color params. Unlike
/// the generic particle-only heroes, three status particles live under
/// `particles/status_fx/`, and a few hero-specific textures are chromatic:
/// a green projected blade-dash self-illum swatch plus the two shadow-redemption
/// status maps. The other Yamato ability textures audited from `pak01` are white
/// alpha masks or grayscale ramps, so they are left particle-tinted. The `pak01`
/// audit patched 234 `.vpcf_c` files cleanly, with 66 color-free helpers skipped.
fn yamato_recipe() -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: "yamato".to_string(),
        particle_prefixes: vec![
            "particles/abilities/yamato/".to_string(),
            "particles/weapon_fx/yamato/".to_string(),
            "particles/status_fx/status_fx_yamato".to_string(),
        ],
        texture_entries: [
            "materials/particle/projected/yamato_blade_dash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c",
            "materials/particle/abilities/yamato/yamato_shadow_redemption_complete_status.vtex_c",
            "materials/particle/abilities/yamato/yamato_shadow_redemption_nokill_status.vtex_c",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect(),
        material_entries: Vec::new(),
        model_entries: Vec::new(),
        preview_texture: Some(
            "materials/particle/abilities/yamato/yamato_shadow_redemption_complete_status.vtex_c"
                .to_string(),
        ),
    }
}

/// Graves (`necro`). Particles carry most of her VFX color (her large ability maps
/// are grayscale, tinted by the particle color), but two ability MODELS hold their
/// color in a material `g_vColorTint` constant rather than a texture, and the
/// gravestone's transmissive glow is a small chromatic texture. See the audit notes
/// at [`recipe_for`]. Her large color maps stay particle-driven (no `texture_entries`
/// beyond the glow); no baked vertex colors.
fn necro_recipe() -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: "necro".to_string(),
        particle_prefixes: vec![
            "particles/abilities/necro/".to_string(),
            "particles/weapon_fx/necro/".to_string(),
            // The held-weapon ambient flame (the green fire on her gun/hand) lives
            // under particles/heroes/, not abilities/weapon_fx; the reference pink
            // recolor edits these too. Without this prefix the held flame stays green.
            "particles/heroes/necro/".to_string(),
        ],
        // Her ability PROPS (the zombie/shambler, the ult jar, the gravestone) carry
        // their green in real albedo + transmissive textures, in-game confirmed as the
        // missing piece (particles + tint constants alone left the 3D props green).
        // These live in the hero material dir but are ability props, not her body
        // skin (head/hand/upper/lower/hair/skirt/bag/eye stay vanilla).
        texture_entries: [
            // shambler = the summoned zombie: albedo (2048, chromatic) + transmissive
            "models/heroes_wip/necro/materials/necro_shambler_color_tga_7b1de566.vtex_c",
            "models/heroes_wip/necro/materials/necro_shambler_vmat_g_tnprtransmissivecolor_337e62d.vtex_c",
            // ult jar (jar of dread) + its glass
            "models/heroes_wip/necro/materials/necro_jar_of_dread_color_tga_7f34b26.vtex_c",
            "models/heroes_wip/necro/materials/necro_jar_glass_color_tga_c6d5a0ec.vtex_c",
            // gravestone: faint-green albedo + the green transmissive glow (the
            // standing model and the destruction fx reference it at two paths).
            "models/heroes_wip/necro/materials/necro_gravestone_color_tga_8a0745c.vtex_c",
            "models/heroes_wip/necro/materials/necro_gravestone_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c",
            "models/abilities/materials/necro_gravestone_destruction_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c",
            // the raised soul-picker hand model (necro_hand): bony albedo + the green
            // transmissive glow around it.
            "models/heroes_wip/necro/materials/necro_hand_color_tga_b2300f7f.vtex_c",
            "models/heroes_wip/necro/materials/necro_hand_vmat_g_tnprtransmissivecolor_c987b5a.vtex_c",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect(),
        // Effect-material tints (g_vColorTint / g_vSelfIllumTint), STAMPED with the
        // brand color (see [`recolor_material_color_bytes`]). The reference pink mod
        // stamps one color on all of these, including the held flaming-hand prop and
        // its aura, which a saturation-preserving recolor leaves vanilla.
        material_entries: vec![
            "models/abilities/materials/necro_pickup_sphere.vmat_c".to_string(),
            "materials/particle/abilities/necro/necro_jar_glass.vmat_c".to_string(),
            "models/abilities/materials/necro_hands.vmat_c".to_string(),
            // the green flame aura around the picker hand is self-illum (the
            // selfillum texture is just a grayscale mask).
            "models/heroes_wip/necro/materials/necro_flame_effect_hand.vmat_c".to_string(),
            "models/heroes_wip/necro/materials/necro_flame_effect.vmat_c".to_string(),
            // The flaming hand she HOLDS as a prop + its glow/aura: the soul-picker
            // hand effect (its g_vColorTint is white but driven by a dynamic
            // expression; we stamp the static value + its yellow g_vSelfIllumTint),
            // the picker effect, and the additive radial-glow aura whose vanilla
            // g_vColorTint is plain white (stamped via the re-encode promotion path).
            "models/heroes_wip/necro/materials/necro_picker_hand_effect.vmat_c".to_string(),
            "models/heroes_wip/necro/materials/necro_picker_effect.vmat_c".to_string(),
            "models/heroes_wip/necro/materials/picker_hand_glow.vmat_c".to_string(),
            // The gravestone's glowing skull / R.I.P. text / cracks: a g_vSelfIllumTint
            // (the necro yellow-green that reads as gold on the bright emissive),
            // masked by a grayscale selfillum texture. The standing stone and its
            // destruction fx carry the tint on the material, not a texture.
            "models/heroes_wip/necro/materials/necro_gravestone.vmat_c".to_string(),
            "models/abilities/materials/necro_gravestone_destruction.vmat_c".to_string(),
        ],
        model_entries: Vec::new(),
        preview_texture: None,
    }
}

/// What a [`recolor_hero_to_addon`] run produced.
#[derive(Debug, Clone, Default)]
pub struct HeroRecolorReport {
    pub codename: String,
    pub hue: f64,
    pub saturation: f64,
    pub value: f64,
    /// `.vpcf_c` files that had at least one color channel changed (packed).
    pub particles_recolored: usize,
    /// `.vpcf_c` files under the prefixes that carry no color param (left alone).
    pub particles_no_color: usize,
    /// `.vpcf_c` files that carry color but could not be patched in place (e.g. a
    /// non-v5 KV3 block the in-place scalar patcher does not handle). Skipped and
    /// left vanilla rather than aborting the whole bake; a nonzero count means the
    /// recolor is partial for this hero.
    pub particles_unpatchable: usize,
    pub textures_recolored: usize,
    /// `.vmat_c` entries whose `g_vColorTint` constant was retinted in place.
    pub materials_recolored: usize,
    /// `.vmat_c` entries that carry a tint but couldn't be patched in place (e.g. a
    /// non-v5 KV3 block, or a ZSTD-compressed binary-blob section: there is no ZSTD
    /// encoder, so it cannot be re-emitted). Blobbed **LZ4** v5 materials ARE handled
    /// now (re-emitted still compressed); they no longer land here.
    pub materials_unpatchable: usize,
    pub models_recolored: usize,
    /// Vertices whose baked color changed across the recolored models.
    pub model_vertices: usize,
    /// Total entries packed into the addon.
    pub total_entries: usize,
}

/// Recolor a hero's full ability-VFX set (particles + color textures + vertex
/// colors) to `hue_deg` and pack it into one addon VPK at `out`, each entry at
/// its base path so the addon overrides the base game in place.
///
/// Entries are read from `vpk` first, then `base` (so an active skin's overriding
/// files win); pass `base = None` to recolor straight from the base pak. Errors
/// if no recipe is pinned for `codename`, or if a recipe texture/model entry is
/// missing from the VPK(s) (a likely path-drift bug) so a silently incomplete
/// addon is never written. Particles that carry no color param are skipped, not
/// an error (most of a hero's `.vpcf_c` are color-free helpers); a color-bearing
/// particle that can't be patched in place (a non-v5 KV3 block) is also skipped
/// (counted in `particles_unpatchable`) rather than aborting the whole bake.
pub fn recolor_hero_to_addon(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    codename: &str,
    recolor: Recolor,
    out: impl AsRef<Path>,
) -> Result<HeroRecolorReport> {
    let recipe = recipe_for(codename).with_context(|| {
        format!(
            "no built-in ability-VFX recolor recipe for hero codename {codename:?} \
             (pinned: bookworm/Paige, necro/Graves, inferno/Infernus, yamato/Yamato, \
             plus particle-only unicorn, gigawatt, vampirebat, wraith)"
        )
    })?;

    let vpks = open_vpks(vpk.as_ref(), base)?;
    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let mut report = HeroRecolorReport {
        codename: recipe.codename.clone(),
        hue: recolor.hue,
        saturation: recolor.saturation,
        value: recolor.value,
        ..Default::default()
    };

    // 1. Particles: every `.vpcf_c` under the recipe prefixes. Color params get
    //    retinted in place; the (many) color-free files are skipped.
    let particle_entries = list_entries(&vpks, &recipe.particle_prefixes, ".vpcf_c");
    for entry in &particle_entries {
        let bytes = read_entry(&vpks, entry)
            .with_context(|| format!("reading particle {entry} (listed but unreadable)"))?;
        match recolor_particle_bytes(&bytes, recolor) {
            Ok(Some(new_bytes)) => {
                packed.push((entry.clone(), new_bytes));
                report.particles_recolored += 1;
            }
            Ok(None) => report.particles_no_color += 1,
            // A particle that carries color but can't be patched in place (a non-v5
            // KV3 block the scalar patcher rejects) is skipped, not fatal: leave it
            // vanilla and keep going so the rest of the hero still recolors.
            Err(e) => {
                report.particles_unpatchable += 1;
                eprintln!("  note: skipping {entry} (left vanilla): {e:#}");
            }
        }
    }

    // 2. Color textures + 3. vertex-color models: explicit recipe entries. A
    //    missing entry is a recipe/path bug, so collect and fail loudly.
    let mut missing: Vec<&str> = Vec::new();

    for entry in &recipe.texture_entries {
        match read_entry(&vpks, entry) {
            Some(bytes) => {
                let new_bytes = crate::recolor::recolor_texture_hue(&bytes, recolor)
                    .with_context(|| format!("recoloring texture {entry}"))?;
                packed.push((entry.clone(), new_bytes));
                report.textures_recolored += 1;
            }
            None => missing.push(entry),
        }
    }

    // Material color-tint constants: retint each `.vmat_c`'s `g_vColorTint` in
    // place. Blobbed LZ4 v5 materials are retinted too (kept compressed, only the
    // changed buffer recompressed; see docs/spike-blobbed-vmat-recolor.md). A
    // material with no patchable tint, or one the in-place patcher still can't reach
    // (a non-v5 block or a ZSTD-blob section), is skipped with a note rather than
    // aborting the whole hero.
    for entry in &recipe.material_entries {
        match read_entry(&vpks, entry) {
            Some(bytes) => match recolor_material_color_bytes(&bytes, recolor) {
                Ok(Some(new_bytes)) => {
                    packed.push((entry.clone(), new_bytes));
                    report.materials_recolored += 1;
                }
                Ok(None) => eprintln!("  note: {entry} has no g_vColorTint constant; skipping"),
                Err(e) => {
                    report.materials_unpatchable += 1;
                    eprintln!("  note: skipping material {entry} (left vanilla): {e:#}");
                }
            },
            None => missing.push(entry),
        }
    }

    for entry in &recipe.model_entries {
        match read_entry(&vpks, entry) {
            Some(bytes) => {
                let (new_bytes, stats) =
                    crate::recolor::recolor_model_vertex_colors(&bytes, recolor)
                        .with_context(|| format!("recoloring model {entry}"))?;
                if stats.buffers_recolored == 0 {
                    // Found but colorless: a recipe over-include, not a path bug.
                    // Skip rather than override the base with an identical model.
                    eprintln!("  note: {entry} has no color-bearing vertex buffer; skipping");
                    continue;
                }
                packed.push((entry.clone(), new_bytes));
                report.models_recolored += 1;
                report.model_vertices += stats.vertices;
            }
            None => missing.push(entry),
        }
    }

    if !missing.is_empty() {
        anyhow::bail!(
            "{} recipe entr{} not found in the given VPK(s) (recipe drift?): {}",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" },
            missing.join(", ")
        );
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    crate::pack(&refs, out.as_ref())
        .with_context(|| format!("packing hero recolor into {}", out.as_ref().display()))?;
    report.total_entries = packed.len();
    Ok(report)
}

/// Render a hero's recolor as a PNG swatch for a live UI preview, without baking
/// the whole addon. Reads the recipe's representative `preview_texture` from the
/// VPK(s) and recolors just its top mip (no lossy re-encode, no pack), so a color
/// picker can repaint as the user drags. The PNG is the design-intent color the
/// full bake will land on. Errors if no recipe is pinned, or the preview texture
/// is missing from the VPK(s).
pub fn recolor_hero_preview_png(
    vpk: impl AsRef<Path>,
    base: Option<&Path>,
    codename: &str,
    recolor: Recolor,
) -> Result<Vec<u8>> {
    let recipe = recipe_for(codename).with_context(|| {
        format!(
            "no built-in ability-VFX recolor recipe for hero codename {codename:?} \
             (pinned: bookworm/Paige, necro/Graves, inferno/Infernus, yamato/Yamato, \
             plus particle-only unicorn, gigawatt, vampirebat, wraith)"
        )
    })?;
    let preview_texture = recipe.preview_texture.as_deref().with_context(|| {
        format!(
            "hero {codename:?} is particle-only (no color texture), so there is no \
             preview swatch to render; bake the addon with `recolor_hero_to_addon` instead"
        )
    })?;
    let vpks = open_vpks(vpk.as_ref(), base)?;
    let bytes = read_entry(&vpks, preview_texture).with_context(|| {
        format!("preview texture {preview_texture} not found in the given VPK(s)")
    })?;
    crate::recolor::recolor_texture_preview_png(&bytes, recolor)
        .context("rendering hero recolor preview")
}

/// Recolor one `.vpcf_c`'s color params to `hue_deg` in place, returning the new
/// bytes, or `None` when the file carries no color param to change.
///
/// Decodes the KV3 value tree, collects an in-place scalar edit for every
/// color/tint-keyed integer array (length 3-4, values 0-255), and applies them
/// with [`morphic::patch_kv3_resource_scalars`] (which preserves the KV3 v5
/// framing, value flags, and typed-array tags a full re-encode would strip,
/// breaking the particle's resource references). Promoted from the in-game-proven
/// `examples/recolor_particles.rs`.
pub fn recolor_particle_bytes(vpcf_bytes: &[u8], recolor: Recolor) -> Result<Option<Vec<u8>>> {
    let value = morphic::decode_kv3_resource(vpcf_bytes)
        .map_err(|e| anyhow::anyhow!("decoding particle KV3: {e}"))?;
    let mut edits: Vec<(Vec<Seg>, i64)> = Vec::new();
    collect_color_edits(&value, &mut Vec::new(), false, recolor, &mut edits);
    if edits.is_empty() {
        return Ok(None);
    }
    let new_bytes = morphic::patch_kv3_resource_scalars(vpcf_bytes, &edits)
        .map_err(|e| anyhow::anyhow!("patching particle color scalars: {e}"))?;
    Ok(Some(new_bytes))
}

/// Recolor a material's `g_vColorTint*` / `g_vSelfIllumTint*` constants by
/// **stamping** them with one absolute brand color (`recolor`'s hue at its
/// saturation/value, via [`crate::recolor::stamp_rgb`]), returning the new bytes,
/// or `None` if the material has no such tint param.
///
/// The third color carrier (after particle params and color textures): an ability
/// effect's color is a flat tint constant (an RGBA `f64` vector). The reference
/// recolor mods stamp ONE brand color on every effect tint, including neutral
/// white ones (e.g. an additive glow's white `g_vColorTint`). A hue-only,
/// saturation-preserving recolor can't colorize a white tint, so this stamps the
/// absolute color instead.
///
/// Two write paths, chosen so the result is engine-loadable:
/// - When every stamped channel is a stored `DOUBLE`, the change is a byte-faithful
///   in-place double patch ([`morphic::patch_kv3_resource_doubles`]), which also
///   handles a blobbed material (re-emitted still compressed).
/// - When a channel is a tagless `DOUBLE_ZERO`/`DOUBLE_ONE` (a neutral 0.0/1.0 with
///   no stored bytes), it cannot be patched in place; the material is fully
///   re-encoded ([`morphic::encode_kv3_resource`], which preserves the texture
///   dependency blocks), promoting the channel to a real double. This fallback is
///   only taken for a **non-blobbed** material: a re-encode emits blobs
///   uncompressed, which the engine misreads.
pub fn recolor_material_color_bytes(
    vmat_bytes: &[u8],
    recolor: Recolor,
) -> Result<Option<Vec<u8>>> {
    let value = morphic::decode_kv3_resource(vmat_bytes)
        .map_err(|e| anyhow::anyhow!("decoding material KV3: {e}"))?;
    if value
        .get("m_vectorParams")
        .and_then(Value::as_array)
        .is_none()
    {
        return Ok(None);
    }
    let target = crate::recolor::stamp_rgb(recolor);

    let edits = stamp_tint_edits(&value, target);
    if edits.is_empty() {
        return Ok(None);
    }

    // Byte-faithful in-place stamp (every channel is a stored double). Handles a
    // blobbed material via the compressed re-emit.
    match morphic::patch_kv3_resource_doubles(vmat_bytes, &edits) {
        Ok(new_bytes) => Ok(Some(new_bytes)),
        // A neutral channel (tagless 0.0/1.0) has no stored bytes to patch, so the
        // in-place patch reports it as "not found". Promote it by re-encoding the
        // whole material -- but only when there is no blob section to mangle.
        Err(_) if !morphic::kv3_resource_has_blobs(vmat_bytes).unwrap_or(true) => {
            let mut tree = value;
            stamp_tint_tree(&mut tree, target);
            let new_bytes = morphic::encode_kv3_resource(vmat_bytes, &tree)
                .map_err(|e| anyhow::anyhow!("re-encoding material to stamp tint: {e}"))?;
            Ok(Some(new_bytes))
        }
        Err(e) => Err(anyhow::anyhow!("patching material color tint: {e}")),
    }
}

/// The set of in-place double edits to stamp `target` (linear 0..1 RGB) onto every
/// `g_vColorTint*` / `g_vSelfIllumTint*` RGB channel that differs from it. A channel
/// already equal to the target is skipped (no-op); alpha (index 3) is never touched.
fn stamp_tint_edits(value: &Value, target: [f64; 3]) -> Vec<(Vec<Seg>, f64)> {
    let mut edits = Vec::new();
    let Some(params) = value.get("m_vectorParams").and_then(Value::as_array) else {
        return edits;
    };
    for (i, param) in params.iter().enumerate() {
        if !is_tint_param(param) {
            continue;
        }
        let Some(rgba) = param.get("m_value").and_then(Value::as_array) else {
            continue;
        };
        for (k, &t) in target.iter().enumerate() {
            let Some(orig) = rgba.get(k).and_then(Value::as_f64) else {
                continue;
            };
            if (orig - t).abs() <= f64::EPSILON {
                continue; // already the brand color (e.g. a tint whose R is already 1.0)
            }
            edits.push((
                vec![
                    Seg::Key("m_vectorParams".to_string()),
                    Seg::Index(i),
                    Seg::Key("m_value".to_string()),
                    Seg::Index(k),
                ],
                t,
            ));
        }
    }
    edits
}

/// Set the RGB of every tint param's `m_value` to `target` directly in the decoded
/// tree (the re-encode path, which can promote a tagless 0.0/1.0 to a real double).
fn stamp_tint_tree(value: &mut Value, target: [f64; 3]) {
    let Some(Value::Array(params)) = value.get_mut("m_vectorParams") else {
        return;
    };
    for param in params.iter_mut() {
        if !is_tint_param(param) {
            continue;
        }
        if let Some(Value::Array(rgba)) = param.get_mut("m_value") {
            for (k, &t) in target.iter().enumerate() {
                if let Some(ch) = rgba.get_mut(k) {
                    *ch = Value::Double(t);
                }
            }
        }
    }
}

/// True for a `g_vColorTint*` / `g_vSelfIllumTint*` vector param (the flat effect
/// tints that carry ability color).
fn is_tint_param(param: &Value) -> bool {
    param
        .get("m_name")
        .and_then(Value::as_str)
        .is_some_and(|n| n.starts_with("g_vColorTint") || n.starts_with("g_vSelfIllumTint"))
}

/// If `v` is a numeric array of length 3-4 in Color32 range, return its RGB ints.
fn as_color(v: &Value) -> Option<[i64; 3]> {
    let Value::Array(items) = v else {
        return None;
    };
    if items.len() != 3 && items.len() != 4 {
        return None;
    }
    let mut ch = [0i64; 3];
    for (i, it) in items.iter().enumerate() {
        let n = match it {
            Value::Int(n) if (0..=255).contains(n) => *n,
            Value::UInt(u) if *u <= 255 => i64::try_from(*u).unwrap_or(0),
            _ => return None,
        };
        if i < 3 {
            ch[i] = n;
        }
    }
    Some(ch)
}

/// Apply the shared color set to a Color32 RGB triple (set hue, then scale its
/// saturation and brightness), so a particle param lands on the exact same color
/// as the texture/model recolor.
fn recolored(rgb: [i64; 3], recolor: Recolor) -> [i64; 3] {
    let inp = [
        clamp_channel(rgb[0]),
        clamp_channel(rgb[1]),
        clamp_channel(rgb[2]),
    ];
    let out = set_color(inp, recolor.hue, recolor.saturation, recolor.value);
    [i64::from(out[0]), i64::from(out[1]), i64::from(out[2])]
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn clamp_channel(n: i64) -> u8 {
    n.clamp(0, 255) as u8
}

/// Walk the value tree, building scalar edits for color channels. `path` is the
/// path to the current value; `colorish` is true when reached via a color/tint
/// key. Mirrors `examples/recolor_particles.rs` (the in-game-proven walk).
fn collect_color_edits(
    v: &Value,
    path: &mut Vec<Seg>,
    colorish: bool,
    recolor: Recolor,
    edits: &mut Vec<(Vec<Seg>, i64)>,
) {
    if colorish {
        if let Some(rgb) = as_color(v) {
            let new = recolored(rgb, recolor);
            for (i, &nv) in new.iter().enumerate() {
                if nv != rgb[i] {
                    let mut p = path.clone();
                    p.push(Seg::Index(i));
                    edits.push((p, nv));
                }
            }
            return; // a color array has no colorish children
        }
    }
    match v {
        Value::Object(pairs) => {
            for (k, child) in pairs {
                let kl = k.to_lowercase();
                let c = kl.contains("color") || kl.contains("tint");
                path.push(Seg::Key(k.clone()));
                collect_color_edits(child, path, c, recolor, edits);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                collect_color_edits(item, path, false, recolor, edits);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Open the VPKs in resolution priority order: `vpk` first (a skin's overrides
/// win), then the base pak. Mirrors `model::open_vpks`.
fn open_vpks(vpk: &Path, base: Option<&Path>) -> Result<Vec<valve_pak::VPK>> {
    let mut vpks =
        vec![valve_pak::open(vpk).with_context(|| format!("opening {}", vpk.display()))?];
    if let Some(base) = base {
        vpks.push(valve_pak::open(base).with_context(|| format!("opening {}", base.display()))?);
    }
    Ok(vpks)
}

/// Read a VPK entry from the first of `vpks` that contains it.
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

/// Every entry across `vpks` that ends with `suffix` and starts with any of
/// `prefixes`, de-duplicated and sorted (so a skin override and the base copy of
/// the same path are listed once; [`read_entry`] resolves which wins).
fn list_entries(vpks: &[valve_pak::VPK], prefixes: &[String], suffix: &str) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for vpk in vpks {
        for p in vpk.file_paths() {
            if p.ends_with(suffix) && prefixes.iter().any(|pre| p.starts_with(pre.as_str())) {
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
    fn paige_recipe_is_pinned() {
        let r = recipe_for("bookworm").expect("paige recipe");
        assert_eq!(r.codename, "bookworm");
        assert_eq!(r.particle_prefixes.len(), 2);
        assert_eq!(r.texture_entries.len(), 9);
        assert_eq!(r.model_entries.len(), 2);
        // the preview swatch must be one of the recipe's real texture entries
        let preview = r.preview_texture.expect("paige has a preview texture");
        assert!(r.texture_entries.contains(&preview));
        // codename lookup is case-insensitive
        assert!(recipe_for("BOOKWORM").is_some());
        assert!(recipe_for("hornet").is_none());
    }

    #[test]
    fn celeste_recipe_is_particle_only() {
        let r = recipe_for("unicorn").expect("celeste recipe");
        assert_eq!(r.codename, "unicorn");
        assert_eq!(
            r.particle_prefixes,
            [
                "particles/abilities/unicorn/",
                "particles/weapon_fx/unicorn/"
            ]
        );
        // particle-only: no color textures, no vertex-color models, no swatch
        assert!(r.texture_entries.is_empty());
        assert!(r.model_entries.is_empty());
        assert!(r.preview_texture.is_none());
        // codename lookup is case-insensitive
        assert!(recipe_for("UNICORN").is_some());
    }

    #[test]
    fn inferno_recipe_is_particle_only() {
        // Infernus is recolored by particle params alone: matching the reference, we
        // do NOT touch inferno_body (the body tint does not color his fire), and the
        // fire textures are shared game-wide so they can't be recolored in place yet.
        let r = recipe_for("inferno").expect("inferno recipe");
        assert_eq!(r.codename, "inferno");
        assert_eq!(
            r.particle_prefixes,
            [
                "particles/abilities/inferno/",
                "particles/weapon_fx/inferno/"
            ]
        );
        assert!(
            r.material_entries.is_empty(),
            "no inferno_body (unmatching)"
        );
        assert!(r.texture_entries.is_empty());
        assert!(r.model_entries.is_empty());
        assert!(r.preview_texture.is_none());
        assert!(recipe_for("INFERNO").is_some());
    }

    #[test]
    fn particle_only_heroes_are_pinned() {
        // Seven/Mina/Wraith: all particle-only, same shape as Celeste, prefixes
        // derived from the codename. Hue is supplied at recolor time, so the recipe
        // itself carries no color. (Graves/necro, Infernus, and Yamato are NOT here:
        // they have their own recipes.)
        for code in ["gigawatt", "vampirebat", "wraith"] {
            let r = recipe_for(code).unwrap_or_else(|| panic!("recipe for {code}"));
            assert_eq!(r.codename, code);
            assert_eq!(
                r.particle_prefixes,
                [
                    format!("particles/abilities/{code}/"),
                    format!("particles/weapon_fx/{code}/"),
                ]
            );
            assert!(
                r.texture_entries.is_empty(),
                "{code} should be particle-only"
            );
            assert!(
                r.material_entries.is_empty(),
                "{code} should be particle-only"
            );
            assert!(r.model_entries.is_empty(), "{code} should be particle-only");
            assert!(r.preview_texture.is_none());
        }
        // case-insensitive, and a hero with no pinned recipe still returns None
        assert!(recipe_for("GIGAWATT").is_some());
        assert!(recipe_for("hornet").is_none());
    }

    #[test]
    fn yamato_recipe_adds_status_particles_and_chromatic_textures() {
        let r = recipe_for("yamato").expect("yamato recipe");
        assert_eq!(r.codename, "yamato");
        assert_eq!(
            r.particle_prefixes,
            [
                "particles/abilities/yamato/",
                "particles/weapon_fx/yamato/",
                "particles/status_fx/status_fx_yamato",
            ]
        );
        assert_eq!(r.texture_entries.len(), 3);
        assert!(r
            .texture_entries
            .iter()
            .any(|t| t.contains("yamato_blade_dash_ground_projected")));
        assert!(r
            .texture_entries
            .iter()
            .any(|t| t.contains("shadow_redemption_complete_status")));
        assert!(r
            .texture_entries
            .iter()
            .any(|t| t.contains("shadow_redemption_nokill_status")));
        assert!(r.material_entries.is_empty());
        assert!(r.model_entries.is_empty());
        let preview = r.preview_texture.expect("yamato has a preview texture");
        assert!(r.texture_entries.contains(&preview));
        assert!(recipe_for("YAMATO").is_some());
    }

    #[test]
    fn graves_recipe_adds_glow_texture_and_tint_materials() {
        // Graves (necro): particles (incl. the held-weapon ambient flame under
        // particles/heroes/) + ability-prop color textures + the stamped effect-tint
        // materials, including the held flaming-hand prop and its aura. NOT particle-only.
        let r = recipe_for("necro").expect("necro recipe");
        assert_eq!(r.codename, "necro");
        assert_eq!(r.particle_prefixes.len(), 3);
        assert!(r
            .particle_prefixes
            .iter()
            .any(|p| p == "particles/heroes/necro/"));
        // ability-prop albedo/transmissive textures (shambler/jar/gravestone)
        assert!(r.texture_entries.len() >= 4, "ability-prop color textures");
        assert!(r.texture_entries.iter().any(|t| t.contains("shambler")));
        assert!(r.texture_entries.iter().any(|t| t.contains("jar_of_dread")));
        assert_eq!(
            r.material_entries.len(),
            10,
            "pickup sphere + jar + necro_hands + 2 flame effects + picker hand effect + \
             picker effect + glow aura + 2 gravestone (standing + destruction)"
        );
        assert!(r.material_entries.iter().all(|m| m.ends_with(".vmat_c")));
        // the held flaming-hand prop + its aura + the gravestone glow are present
        assert!(r
            .material_entries
            .iter()
            .any(|m| m.ends_with("necro_picker_hand_effect.vmat_c")));
        assert!(r
            .material_entries
            .iter()
            .any(|m| m.ends_with("picker_hand_glow.vmat_c")));
        assert!(r
            .material_entries
            .iter()
            .any(|m| m.ends_with("necro_gravestone.vmat_c")));
        assert!(r.model_entries.is_empty());
    }

    #[test]
    fn as_color_accepts_color32_arrays_only() {
        assert_eq!(
            as_color(&Value::Array(vec![
                Value::Int(0),
                Value::Int(255),
                Value::Int(148)
            ])),
            Some([0, 255, 148])
        );
        // RGBA: alpha is ignored, RGB returned
        assert_eq!(
            as_color(&Value::Array(vec![
                Value::Int(10),
                Value::Int(20),
                Value::Int(30),
                Value::Int(255),
            ])),
            Some([10, 20, 30])
        );
        // out of range / wrong length / non-int -> not a color
        assert_eq!(
            as_color(&Value::Array(vec![
                Value::Int(0),
                Value::Int(300),
                Value::Int(0)
            ])),
            None
        );
        assert_eq!(
            as_color(&Value::Array(vec![Value::Int(1), Value::Int(2)])),
            None
        );
    }

    #[test]
    fn recolored_matches_the_documented_purple() {
        // The same fully-saturated green -> hue 280 -> purple the in-game recolor
        // produced, identical to the texture/model `set_hue`. A hue-only recolor
        // (unit saturation + value) reproduces the original behavior.
        assert_eq!(recolored([0, 255, 148], Recolor::hue(280.0)), [170, 0, 255]);
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../morphic/fixtures/material/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|_| panic!("read material fixture {name}"))
    }

    fn tint_rgb(bytes: &[u8], name: &str) -> [f64; 3] {
        let tree = morphic::decode_kv3_resource(bytes).expect("decode stamped material");
        let params = tree
            .get("m_vectorParams")
            .and_then(Value::as_array)
            .expect("m_vectorParams");
        let p = params
            .iter()
            .find(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
            .unwrap_or_else(|| panic!("no {name}"));
        let v = p.get("m_value").and_then(Value::as_array).expect("m_value");
        [
            v[0].as_f64().unwrap(),
            v[1].as_f64().unwrap(),
            v[2].as_f64().unwrap(),
        ]
    }

    #[test]
    fn stamps_an_absolute_brand_color_on_neutral_and_blobbed_tints() {
        // Hue 328 stamps a vivid pink: hsv(328,1,1) = (1.0, 0.0, 0.533...).
        let recolor = Recolor::new(328.0, 1.0, 1.0);
        let target = [1.0, 0.0, 0.533_333_3];
        let close = |got: [f64; 3]| (0..3).all(|k| (got[k] - target[k]).abs() < 1e-5);

        // 1. A neutral WHITE tint (picker_hand_glow g_vColorTint = [1,1,1], stored
        //    tagless) can't be patched in place, so it takes the re-encode promotion
        //    path -- and still ends up the brand color.
        let glow = recolor_material_color_bytes(&fixture("picker_hand_glow.vmat_c"), recolor)
            .expect("stamp glow")
            .expect("glow has a tint");
        assert!(
            close(tint_rgb(&glow, "g_vColorTint1")),
            "white aura stamped pink"
        );

        // 2. A blobbed material with colored tints (necro_hands) stamps in place and
        //    stays a compressed, blobbed, engine-loadable block.
        let hands = recolor_material_color_bytes(&fixture("necro_hands.vmat_c"), recolor)
            .expect("stamp hands")
            .expect("hands has a tint");
        assert!(close(tint_rgb(&hands, "g_vColorTint1")));
        assert!(close(tint_rgb(&hands, "g_vSelfIllumTint1")));
        assert!(
            morphic::kv3_resource_has_blobs(&hands).unwrap(),
            "necro_hands stays blobbed (in-place stamp, not flattened)"
        );

        // 3. The two-blob held-hand material (its g_vColorTint is white-but-stored,
        //    its g_vSelfIllumTint colored) stamps in place too, after the blob-frame
        //    reader fix lets it decode at all.
        let held =
            recolor_material_color_bytes(&fixture("necro_picker_hand_effect.vmat_c"), recolor)
                .expect("stamp held hand")
                .expect("held hand has a tint");
        assert!(close(tint_rgb(&held, "g_vColorTint1")));
        assert!(close(tint_rgb(&held, "g_vSelfIllumTint1")));
    }
}
