//! Animation frontier round 3. Rounds 1-2 confirmed: path overrides, in-list
//! and out-of-list graph redirects, duration slow-mo, and hitboxes following
//! modded bones in local sim.
//!
//! Experiment E, cross-hero clip loading: redirect Viscous's out-of-combat
//! stand idle to KELVIN's clip path (shared skeleton, different hero dir).
//! Tests whether the resource loader cares whose directory a clip lives in.
//!
//! Experiment F, anim-sound remap: in `abrams_anim_sounds.vdata_c`, point the
//! footstep entry's soundevent at the melee-swing soundevent. Every Abrams
//! step should whoosh. First in-game test of the tagged_sounds vdata route.
//!
//! Experiment G, interrupt-layer override: replace Abrams' three flinch clips
//! with his `ui_hero_select` pose clip. Shooting him should snap him into the
//! select pose. Tests whether interrupt-layer clips override like base-layer.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_experiments3 -- \
//!     <pak01_dir.vpk> <out_dir>

use anyhow::{bail, Context, Result};
use morphic::kv3::{Seg, Value};

const VISCOUS_GRAPH: &str = "animgraphs/animgraph2/hero/viscous/viscous_loco_set_idle.vnmgraph_c";
const KELVIN_IDLE: &str = "models/heroes_staging/kelvin_v2/clip/weapon_stand_idle.vnmclip";

const ABRAMS_SOUNDS: &str = "scripts/tagged_sounds/abrams_anim_sounds.vdata_c";

const ABRAMS_POSE: &str = "models/heroes_wip/abrams/clips/ui_hero_select.vnmclip_c";
const ABRAMS_FLINCHES: [&str; 3] = [
    "models/heroes_wip/abrams/clips/flinch_back.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_left.vnmclip_c",
    "models/heroes_wip/abrams/clips/flinch_right.vnmclip_c",
];

fn experiment_e(pak: &str, out_dir: &str) -> Result<()> {
    let graph = vpkmerge_core::read_vpk_entry(pak, VISCOUS_GRAPH)?;
    let root = morphic::decode_kv3_resource(&graph).context("decoding viscous idle graph")?;
    let Some(Value::Array(resources)) = root.get("m_resources") else {
        bail!("no m_resources in viscous graph");
    };
    let mut from_idx = None;
    println!("[E] viscous idle graph resources:");
    for (i, r) in resources.iter().enumerate() {
        if let Value::String(s) = r {
            println!("    [{i}] {s}");
            if s.contains("/weapon_stand_idle") {
                from_idx = Some(i);
            }
        }
    }
    let from_idx = from_idx.context("no weapon_stand_idle in viscous graph")?;

    let edits = vec![(
        vec![Seg::Key("m_resources".into()), Seg::Index(from_idx)],
        KELVIN_IDLE.to_string(),
    )];
    let patched = morphic::patch_kv3_resource_strings_adding(&graph, &edits)
        .context("patching viscous graph with kelvin clip")?;
    let check = morphic::decode_kv3_resource(&patched)?;
    match check.get("m_resources") {
        Some(Value::Array(rs)) if matches!(rs.get(from_idx), Some(Value::String(s)) if s == KELVIN_IDLE) =>
        {
            println!("[E] redirect verified: viscous idle -> {KELVIN_IDLE} (cross-hero)");
        }
        _ => bail!("viscous graph patch did not verify"),
    }

    let out = format!("{out_dir}/pak_expE_viscous_kelvin_idle_dir.vpk");
    vpkmerge_core::pack(&[(VISCOUS_GRAPH, patched.as_slice())], &out)?;
    println!("[E] -> {out}");
    Ok(())
}

fn experiment_f(pak: &str, out_dir: &str) -> Result<()> {
    let vdata = vpkmerge_core::read_vpk_entry(pak, ABRAMS_SOUNDS)?;
    let root = morphic::decode_kv3_resource(&vdata).context("decoding abrams anim sounds")?;
    let Value::Object(entries) = &root else {
        bail!("anim sounds root is not an object");
    };

    let mut footstep_event = None;
    let mut swing_event = None;
    println!("[F] abrams anim sound bindings:");
    for (name, v) in entries {
        if let Some(Value::String(se)) = v.get("m_soundEvent") {
            println!("    {name} -> {se}");
            if name == "footstep" {
                footstep_event = Some(se.clone());
            }
            if name == "melee_swing" {
                swing_event = Some(se.clone());
            }
        }
    }
    let footstep_event = footstep_event.context("no footstep binding found")?;
    let swing_event = swing_event.context("no melee_swing binding found")?;

    let edits = vec![(
        vec![Seg::Key("footstep".into()), Seg::Key("m_soundEvent".into())],
        swing_event.clone(),
    )];
    let patched = morphic::patch_kv3_resource_strings_adding(&vdata, &edits)
        .context("patching footstep soundevent")?;
    let check = morphic::decode_kv3_resource(&patched)?;
    match check.get("footstep").and_then(|f| f.get("m_soundEvent")) {
        Some(Value::String(s)) if *s == swing_event => {
            println!("[F] footstep remapped: {footstep_event} -> {swing_event}");
        }
        other => bail!("footstep patch did not verify: {other:?}"),
    }

    let out = format!("{out_dir}/pak_expF_abrams_whoosh_steps_dir.vpk");
    vpkmerge_core::pack(&[(ABRAMS_SOUNDS, patched.as_slice())], &out)?;
    println!("[F] -> {out}");
    Ok(())
}

fn experiment_g(pak: &str, out_dir: &str) -> Result<()> {
    let pose = vpkmerge_core::read_vpk_entry(pak, ABRAMS_POSE)?;
    let files: Vec<(&str, &[u8])> = ABRAMS_FLINCHES
        .iter()
        .map(|entry| (*entry, pose.as_slice()))
        .collect();
    let out = format!("{out_dir}/pak_expG_abrams_flinch_pose_dir.vpk");
    vpkmerge_core::pack(&files, &out)?;
    println!("[G] flinch x{} -> select pose -> {out}", files.len());
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    experiment_e(&pak, &out_dir)?;
    experiment_f(&pak, &out_dir)?;
    experiment_g(&pak, &out_dir)?;
    Ok(())
}
