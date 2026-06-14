//! Animation frontier experiments 2 and 3 (experiment 1, the Viscous/Kelvin
//! pose transplant, is in `pose_transplant.rs` and passed in-game).
//!
//! Experiment A, proto-taunt: override Yamato's standing reload clips with her
//! own `ui_hero_select` pose clip via plain path override. If the engine plays
//! it in a match, "press R to strike a pose" works and the taunt pipeline is
//! proven end to end.
//!
//! Experiment B, animgraph redirect: patch one clip-path string inside
//! `abrams_loco_set_idle.vnmgraph_c` so the out-of-combat stand idle loads the
//! crouch idle clip instead. Tests whether graphs follow their `m_resources`
//! strings, the mechanism a real taunt system would use.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_experiments -- \
//!     <pak01_dir.vpk> <out_dir>
//! Writes <out_dir>/pak_expA_yamato_reload_pose_dir.vpk and
//! <out_dir>/pak_expB_abrams_idle_redirect_dir.vpk.

use anyhow::{bail, Context, Result};
use morphic::kv3::{Seg, Value};

const YAMATO_POSE: &str = "models/heroes_wip/yamato/clips/ui_hero_select.vnmclip_c";
const YAMATO_RELOADS: [&str; 2] = [
    "models/heroes_wip/yamato/clips/reload_idle.vnmclip_c",
    "models/heroes_wip/yamato/clips/reload_idle_quick.vnmclip_c",
];

const ABRAMS_GRAPH: &str = "animgraphs/animgraph2/hero/abrams/abrams_loco_set_idle.vnmgraph_c";
const ABRAMS_FROM: &str = "models/heroes_wip/abrams/clips/out_of_combat_stand_idle.vnmclip";
const ABRAMS_TO: &str = "models/heroes_wip/abrams/clips/weapon_crouch_idle.vnmclip";

fn experiment_a(pak: &str, out_dir: &str) -> Result<()> {
    let pose = vpkmerge_core::read_vpk_entry(pak, YAMATO_POSE)?;
    let root = morphic::decode_kv3_resource(&pose).context("decoding yamato pose clip")?;
    let additive = root
        .get("m_bIsAdditive")
        .is_some_and(|v| matches!(v, Value::Bool(true)));
    if additive {
        bail!("yamato ui_hero_select is additive; not a standalone pose");
    }

    let files: Vec<(&str, &[u8])> = YAMATO_RELOADS
        .iter()
        .map(|entry| (*entry, pose.as_slice()))
        .collect();
    let out = format!("{out_dir}/pak_expA_yamato_reload_pose_dir.vpk");
    vpkmerge_core::pack(&files, &out)?;
    println!(
        "[A] yamato reload -> select pose ({} entries) -> {out}",
        files.len()
    );
    Ok(())
}

fn experiment_b(pak: &str, out_dir: &str) -> Result<()> {
    let graph = vpkmerge_core::read_vpk_entry(pak, ABRAMS_GRAPH)?;
    let root = morphic::decode_kv3_resource(&graph).context("decoding abrams idle graph")?;
    let Some(Value::Array(resources)) = root.get("m_resources") else {
        bail!("abrams idle graph has no m_resources array");
    };
    println!("[B] graph references {} resources:", resources.len());
    let mut from_idx = None;
    for (i, r) in resources.iter().enumerate() {
        if let Value::String(s) = r {
            println!("    [{i}] {s}");
            if s == ABRAMS_FROM {
                from_idx = Some(i);
            }
            if s == ABRAMS_TO {
                println!("    (redirect target [{i}] already referenced; good)");
            }
        }
    }
    let from_idx = from_idx.context("stand-idle clip path not found in m_resources")?;

    let edits = vec![(
        vec![Seg::Key("m_resources".into()), Seg::Index(from_idx)],
        ABRAMS_TO.to_string(),
    )];
    let patched = morphic::patch_kv3_resource_strings_adding(&graph, &edits)
        .context("patching graph resource string")?;

    // Round-trip check: the patched graph must decode and show the redirect.
    let check = morphic::decode_kv3_resource(&patched).context("re-decoding patched graph")?;
    match check.get("m_resources") {
        Some(Value::Array(rs)) => match rs.get(from_idx) {
            Some(Value::String(s)) if s == ABRAMS_TO => {
                println!("[B] redirect verified: m_resources[{from_idx}] now {ABRAMS_TO}");
            }
            other => bail!("patched graph entry unexpected: {other:?}"),
        },
        _ => bail!("patched graph lost m_resources"),
    }

    let out = format!("{out_dir}/pak_expB_abrams_idle_redirect_dir.vpk");
    vpkmerge_core::pack(&[(ABRAMS_GRAPH, patched.as_slice())], &out)?;
    println!("[B] abrams stand idle -> crouch idle redirect -> {out}");
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    experiment_a(&pak, &out_dir)?;
    experiment_b(&pak, &out_dir)?;
    Ok(())
}
