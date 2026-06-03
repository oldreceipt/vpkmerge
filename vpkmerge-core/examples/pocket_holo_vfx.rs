// Pocket "Holo Satchel" matching ability VFX.
//
// Companion to the installed Holo Satchel skin (trippy-skin --hero pocket --style
// holo, body + weapons, intensity 1.00, scroll 1.15 -- shipped as addons/pak09).
// That skin only repaints Pocket's body/weapon model textures; his ability and
// weapon-tracer particles stay vanilla. This builder paints the ability side to
// match: every color-bearing Pocket particle gets a holo-sampled hue, the three
// Pocket ability textures (satchel-projected / body-color / magic-missile illum)
// get the same procedural holo pattern, and the high-visibility particles
// (glow/beam/trail/arc/ring) get the animated `cycle` pass so the spectrum sweeps
// over each particle's lifetime -- echoing the skin's 1.15 UV scroll.
//
// This is a thin, re-runnable record of how addons/pak10 was produced. It calls
// the same core path as `vpkmerge trippy-vfx --hero pocket --style holo --targets
// all --intensity 1.0 --animation-style cycle --animation-intensity 1.15`. Ability
// particles are vanilla, so it reads straight from base pak01 (no skin VPK needed).
//
// usage:
//   cargo run --release --example pocket_holo_vfx
//   cargo run --release --example pocket_holo_vfx -- <pak01_dir.vpk> <out_dir.vpk>
//   cargo run --release --example pocket_holo_vfx -- <pak01_dir.vpk> <out_dir.vpk> \
//       [--style holo] [--animation-style cycle] [--animation-intensity 1.15] [--intensity 1.0]
//
// Then drop the output in over the game: cp <out_dir.vpk> \
//   ~/.local/share/Steam/steamapps/common/Deadlock/game/citadel/addons/pak10_dir.vpk
use anyhow::{Context, Result};
use std::path::PathBuf;
use vpkmerge_core::{
    trippy_ability_vfx_to_addon, PrismAnimationStyle, TrippyAbilityOptions, TrippyStyle,
};

const DEFAULT_PAK01: &str =
    "/home/esoc/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk";
const DEFAULT_OUT: &str = "target/grimoire-trippy-roster/pocket_holo_vfx_dir.vpk";

fn parse_animation(name: &str, intensity: f64) -> Result<(PrismAnimationStyle, f64)> {
    match name.to_ascii_lowercase().as_str() {
        "off" | "none" | "static" => Ok((PrismAnimationStyle::Sweep, 0.0)),
        "sweep" => Ok((PrismAnimationStyle::Sweep, intensity)),
        "loop" | "loops" => Ok((PrismAnimationStyle::Loop, intensity)),
        "cycle" | "cycles" => Ok((PrismAnimationStyle::Cycle, intensity)),
        other => {
            anyhow::bail!("--animation-style must be off, sweep, loop, or cycle (got {other:?})")
        }
    }
}

fn main() -> Result<()> {
    let mut positional: Vec<String> = Vec::new();
    let mut style = "holo".to_string();
    let mut animation_style = "cycle".to_string();
    let mut animation_intensity: f64 = 1.15;
    let mut intensity: f32 = 1.0;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--style" => style = args.next().context("--style needs a value")?,
            "--animation-style" => {
                animation_style = args.next().context("--animation-style needs a value")?;
            }
            "--animation-intensity" => {
                animation_intensity = args
                    .next()
                    .context("--animation-intensity needs a value")?
                    .parse()
                    .context("--animation-intensity must be a number")?;
            }
            "--intensity" => {
                intensity = args
                    .next()
                    .context("--intensity needs a value")?
                    .parse()
                    .context("--intensity must be a number")?;
            }
            other => positional.push(other.to_string()),
        }
    }

    let pak01 = PathBuf::from(positional.first().map_or(DEFAULT_PAK01, String::as_str));
    let out = PathBuf::from(positional.get(1).map_or(DEFAULT_OUT, String::as_str));

    let (animation_style, animation_intensity) =
        parse_animation(&animation_style, animation_intensity)?;
    let options = TrippyAbilityOptions {
        style: TrippyStyle::from_name(&style)?,
        intensity,
        phase: 0.0,
        animation_intensity,
        animation_style,
        include_abilities: true,
        include_weapons: true,
    };

    // Ability particles are vanilla, so read straight from base pak01 (no skin VPK
    // fallback needed). The recipe pins Pocket's particle prefixes + the 3 ability
    // textures; see hero_recolor::pocket_recipe.
    let report = trippy_ability_vfx_to_addon(&pak01, None, "pocket", &options, &out)
        .context("building Pocket holo ability VFX")?;

    eprintln!(
        "pocket: {}/{} particle(s) holo-recolored ({} color-free, {} unpatchable), {} texture(s) painted",
        report.particles_recolored,
        report.particles_total,
        report.particles_no_color,
        report.particles_unpatchable,
        report.textures_painted,
    );
    if animation_intensity > 0.0 {
        eprintln!(
            "  animated: {} particle(s) ({} texture-age input(s), {} scroll multiplier(s), {} gradient timing edit(s), {} color-gradient loop(s), {} color-cycle operator(s))",
            report.particles_animated,
            report.texture_age_inputs,
            report.texture_offset_multipliers,
            report.gradient_timing_edits,
            report.color_gradient_loops,
            report.color_cycle_operators,
        );
    }
    println!(
        "wrote {}: {} entries, hero pocket painted with {} trippy ability VFX",
        out.display(),
        report.total_entries,
        options.style.as_str(),
    );
    Ok(())
}
