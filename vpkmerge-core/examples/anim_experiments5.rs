//! Animation frontier round 4: unfakeable signals only.
//!
//! Experiment E3, missing-path injection: Viscous's graphs reference
//! `models/heroes_wip/viscous/clips/weapon_stand_idle.vnmclip`, which does
//! not exist in pak01 (the engine resolves his anims via fallback, likely
//! Kelvin's library). Pack Kelvin's `knockdown_large_loop` bytes AT that
//! missing path. If the engine prefers a now-existing file over its
//! fallback, idle Viscous lies flat on the ground: proof that new clips can
//! be added at referenced-but-missing paths without touching any graph, the
//! cleanest possible taunt-injection mechanism.
//!
//! Experiment G3, interrupt layer with a readable payload: Abrams' three
//! flinch clips -> his own `knockdown_large_loop`. Rounds G/G2 failed only
//! because a 0.3s flinch window can't show a standing pose; dropping flat
//! reads instantly. Run WITHOUT the knockdown-idle addon (pak73) installed.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_experiments5 -- \
//!     <pak01_dir.vpk> <out_dir>

use anyhow::{Context, Result};

const VISCOUS_MISSING_IDLE: &str = "models/heroes_wip/viscous/clips/weapon_stand_idle.vnmclip_c";
const KELVIN_KNOCKDOWN: &str =
    "models/heroes_staging/kelvin_v2/clip/knockdown_large_loop.vnmclip_c";

const ABRAMS_KNOCKDOWN: &str = "models/heroes_wip/abrams/clips/knockdown_large_loop.vnmclip_c";
const ABRAMS_FLINCHES: [&str; 3] = [
    "models/heroes_wip/abrams/clips/flinch_back.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_left.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_right.vnmclip_c",
];

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    // E3: inject a clip at the referenced-but-missing path.
    if vpkmerge_core::read_vpk_entry(&pak, VISCOUS_MISSING_IDLE).is_ok() {
        println!("[E3] warning: {VISCOUS_MISSING_IDLE} exists now; not a missing-path test");
    }
    let payload = vpkmerge_core::read_vpk_entry(&pak, KELVIN_KNOCKDOWN)?;
    let out_e = format!("{out_dir}/pak_expE3_viscous_injected_idle_dir.vpk");
    vpkmerge_core::pack(&[(VISCOUS_MISSING_IDLE, payload.as_slice())], &out_e)?;
    println!("[E3] kelvin knockdown_large_loop injected at missing viscous idle path -> {out_e}");

    // G3: flinch -> knockdown, the readable interrupt payload.
    // Result: NO flinch played at all. The override loaded (vanilla flinch
    // suppressed) but the payload contributed nothing: flinch clips are
    // ADDITIVE (m_bIsAdditive=true, confirmed by anim_clip_scan) and a
    // non-additive full-body clip is inert in an additive blend slot.
    let knockdown = vpkmerge_core::read_vpk_entry(&pak, ABRAMS_KNOCKDOWN)?;
    let files: Vec<(&str, &[u8])> = ABRAMS_FLINCHES
        .iter()
        .map(|entry| (*entry, knockdown.as_slice()))
        .collect();
    let out_g = format!("{out_dir}/pak_expG3_abrams_flinch_knockdown_dir.vpk");
    vpkmerge_core::pack(&files, &out_g)?;
    println!(
        "[G3] flinch x{} -> knockdown_large_loop -> {out_g}",
        files.len()
    );

    // G4: flinch -> landing_impact_idle, an ADDITIVE payload (0.97s heavy
    // landing squash). Like-for-like additive swap; shooting him should show
    // the landing wobble instead of a flinch.
    let landing = vpkmerge_core::read_vpk_entry(
        &pak,
        "models/heroes_wip/abrams/clips/landing_impact_idle.vnmclip_c",
    )?;
    let files: Vec<(&str, &[u8])> = ABRAMS_FLINCHES
        .iter()
        .map(|entry| (*entry, landing.as_slice()))
        .collect();
    let out_g4 = format!("{out_dir}/pak_expG4_abrams_flinch_landing_dir.vpk");
    vpkmerge_core::pack(&files, &out_g4)?;
    println!(
        "[G4] flinch x{} -> landing_impact_idle (additive) -> {out_g4}",
        files.len()
    );
    Ok(())
}
