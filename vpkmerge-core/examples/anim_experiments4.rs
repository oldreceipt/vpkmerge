//! Animation frontier round 3.5: disambiguation of E and G, which came back
//! inconclusive (substitute clips looked too close to the originals).
//!
//! Experiment E2, cross-hero loading, unambiguous edition: first byte-compare
//! Viscous's and Kelvin's `weapon_stand_idle` clips to settle whether the
//! round-3 "looks the same" was the base case. Then redirect Viscous's idle
//! slot to Kelvin's `ui_hero_select` clip, a pose Viscous never plays as an
//! idle (and one we know on sight from the round-1 transplant).
//!
//! Experiment G2, interrupt layer, unambiguous edition: Abrams' three flinch
//! clips -> `sleep_idle` (the standing droopy sleep-debuff pose). Shooting
//! him should snap him to sleep, unmistakable vs a vanilla flinch.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_experiments4 -- \
//!     <pak01_dir.vpk> <out_dir>

use anyhow::{bail, Context, Result};
use morphic::kv3::{Seg, Value};

const VISCOUS_GRAPH: &str = "animgraphs/animgraph2/hero/viscous/viscous_loco_set_idle.vnmgraph_c";
const VISCOUS_IDLE_C: &str = "models/heroes_wip/viscous/clips/weapon_stand_idle.vnmclip_c";
const KELVIN_IDLE_C: &str = "models/heroes_staging/kelvin_v2/clip/weapon_stand_idle.vnmclip_c";
const KELVIN_POSE: &str = "models/heroes_staging/kelvin_v2/clip/ui_hero_select.vnmclip";

const ABRAMS_SLEEP_C: &str = "models/heroes_wip/abrams/clips/sleep_idle.vnmclip_c";
const ABRAMS_FLINCHES: [&str; 3] = [
    "models/heroes_wip/abrams/clips/flinch_back.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_left.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_right.vnmclip_c",
];

fn experiment_e2(pak: &str, out_dir: &str) -> Result<()> {
    // Part 1: was round 3's "looks the same" the base case? Finding: viscous
    // ships only 3 compiled clips; the path his graphs reference does not
    // exist in pak01 at all, so the engine resolves his anims through an
    // indirection (likely filename fallback against the shared skeleton's
    // clip library, i.e. kelvin's). Round 3's redirect to kelvin's file was
    // then a null edit.
    match vpkmerge_core::read_vpk_entry(pak, VISCOUS_IDLE_C) {
        Ok(viscous_idle) => {
            let kelvin_idle = vpkmerge_core::read_vpk_entry(pak, KELVIN_IDLE_C)?;
            println!(
                "[E2] base case: viscous clip exists ({} bytes), kelvin {} bytes, identical: {}",
                viscous_idle.len(),
                kelvin_idle.len(),
                viscous_idle == kelvin_idle
            );
        }
        Err(_) => {
            println!("[E2] base case CONFIRMED: {VISCOUS_IDLE_C} does not exist in pak01;");
            println!("     viscous animates via engine-side resolution (kelvin's library)");
        }
    }

    // Part 2: the unambiguous redirect, idle -> kelvin's select pose.
    let graph = vpkmerge_core::read_vpk_entry(pak, VISCOUS_GRAPH)?;
    let root = morphic::decode_kv3_resource(&graph)?;
    let Some(Value::Array(resources)) = root.get("m_resources") else {
        bail!("no m_resources in viscous graph");
    };
    let from_idx = resources
        .iter()
        .position(|r| matches!(r, Value::String(s) if s.contains("/weapon_stand_idle")))
        .context("no weapon_stand_idle in viscous graph")?;

    let edits = vec![(
        vec![Seg::Key("m_resources".into()), Seg::Index(from_idx)],
        KELVIN_POSE.to_string(),
    )];
    let patched = morphic::patch_kv3_resource_strings_adding(&graph, &edits)?;
    let check = morphic::decode_kv3_resource(&patched)?;
    match check.get("m_resources") {
        Some(Value::Array(rs)) if matches!(rs.get(from_idx), Some(Value::String(s)) if s == KELVIN_POSE) =>
        {
            println!("[E2] redirect verified: viscous weapon idle -> {KELVIN_POSE}");
        }
        _ => bail!("viscous graph patch did not verify"),
    }

    let out = format!("{out_dir}/pak_expE2_viscous_kelvin_pose_idle_dir.vpk");
    vpkmerge_core::pack(&[(VISCOUS_GRAPH, patched.as_slice())], &out)?;
    println!("[E2] -> {out}");
    Ok(())
}

fn experiment_g2(pak: &str, out_dir: &str) -> Result<()> {
    let sleep = vpkmerge_core::read_vpk_entry(pak, ABRAMS_SLEEP_C)?;
    let files: Vec<(&str, &[u8])> = ABRAMS_FLINCHES
        .iter()
        .map(|entry| (*entry, sleep.as_slice()))
        .collect();
    let out = format!("{out_dir}/pak_expG2_abrams_flinch_sleep_dir.vpk");
    vpkmerge_core::pack(&files, &out)?;
    println!("[G2] flinch x{} -> sleep_idle -> {out}", files.len());
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    experiment_e2(&pak, &out_dir)?;
    experiment_g2(&pak, &out_dir)?;
    Ok(())
}
