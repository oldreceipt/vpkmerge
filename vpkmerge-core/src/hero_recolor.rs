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
    match codename.to_lowercase().as_str() {
        "bookworm" => Some(paige_recipe()),
        "unicorn" => Some(celeste_recipe()),
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

/// Celeste (`unicorn`). Particle-only: her ability VFX carry color purely through
/// `.vpcf_c` color params (no color-bearing textures or baked mesh vertex colors),
/// so the recipe is the two `particles/{abilities,weapon_fx}/unicorn/` prefixes and
/// nothing else. Pinned from her in-game pink recolor mod (`pak07`), which retints
/// both namespaces (250 ability + 27 weapon particles); a base-vs-mod scan put the
/// target hue at ~329 deg.
fn celeste_recipe() -> HeroRecolorRecipe {
    HeroRecolorRecipe {
        codename: "unicorn".to_string(),
        particle_prefixes: vec![
            "particles/abilities/unicorn/".to_string(),
            "particles/weapon_fx/unicorn/".to_string(),
        ],
        texture_entries: Vec::new(),
        model_entries: Vec::new(),
        // Particle-only: no color texture to render as a swatch.
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
    pub textures_recolored: usize,
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
/// an error (most of a hero's `.vpcf_c` are color-free helpers).
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
             (only `bookworm` / Paige is pinned so far)"
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
        match recolor_particle_bytes(&bytes, recolor)
            .with_context(|| format!("recoloring particle {entry}"))?
        {
            Some(new_bytes) => {
                packed.push((entry.clone(), new_bytes));
                report.particles_recolored += 1;
            }
            None => report.particles_no_color += 1,
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
             (only `bookworm` / Paige is pinned so far)"
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
}
