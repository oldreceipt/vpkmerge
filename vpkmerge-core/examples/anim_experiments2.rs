//! Animation frontier experiments C and D. A (reload pose taunt) and B
//! (in-list graph redirect) passed in-game; B also showed hitboxes follow the
//! modded bones in locally simulated play (hideout bot took hits at the
//! visual crouch, not the vanilla standing position).
//!
//! Experiment C, out-of-list redirect + hitbox probe: point Abrams'
//! out-of-combat stand idle at `sleep_idle`, a clip NOT in the graph's
//! original `m_resources`. Proves any same-skeleton clip is reachable from
//! any graph slot (the taunt system's real requirement). A sleeping-on-the-
//! ground bot is also the loudest possible hitbox test: shoot the body on
//! the ground vs where it would stand.
//!
//! Experiment D, duration patch: double `m_flDuration` on Abrams' forward
//! weapon run clips. If the engine derives playback rate from duration, he
//! runs in slow motion (feet sliding); if nothing changes, rate comes from
//! frame count and speed-shifting needs the pose codec.
//!
//! Usage: cargo run --release -p vpkmerge-core --example anim_experiments2 -- \
//!     <pak01_dir.vpk> <out_dir>

use anyhow::{bail, Context, Result};
use morphic::kv3::{Seg, Value};

const ABRAMS_GRAPH: &str = "animgraphs/animgraph2/hero/abrams/abrams_loco_set_idle.vnmgraph_c";
const ABRAMS_FROM: &str = "models/heroes_wip/abrams/clips/out_of_combat_stand_idle.vnmclip";
// Round 1 used sleep_idle, which turned out to be a STANDING sleep (the
// in-game sleep-debuff pose), so it proved the out-of-list redirect but not
// the hitbox displacement. knockdown_large_loop is genuinely on the ground.
const ABRAMS_TARGET: &str = "models/heroes_wip/abrams/clips/knockdown_large_loop.vnmclip";

const ABRAMS_RUNS: [&str; 2] = [
    "models/heroes_wip/abrams/clips/weapon_run_n.vnmclip_c",
    "models/heroes_wip/abrams/clips/weapon_run_center.vnmclip_c",
];

fn read_duration(bytes: &[u8]) -> Result<f64> {
    let root = morphic::decode_kv3_resource(bytes)?;
    match root.get("m_flDuration") {
        Some(Value::Double(d)) => Ok(*d),
        other => bail!("m_flDuration not a double: {other:?}"),
    }
}

fn experiment_c(pak: &str, out_dir: &str) -> Result<()> {
    let graph = vpkmerge_core::read_vpk_entry(pak, ABRAMS_GRAPH)?;
    let root = morphic::decode_kv3_resource(&graph).context("decoding abrams idle graph")?;
    let Some(Value::Array(resources)) = root.get("m_resources") else {
        bail!("no m_resources");
    };
    let from_idx = resources
        .iter()
        .position(|r| matches!(r, Value::String(s) if s == ABRAMS_FROM))
        .context("stand-idle path not in m_resources")?;
    if resources
        .iter()
        .any(|r| matches!(r, Value::String(s) if s == ABRAMS_TARGET))
    {
        bail!("target clip already referenced; not an out-of-list test");
    }

    let edits = vec![(
        vec![Seg::Key("m_resources".into()), Seg::Index(from_idx)],
        ABRAMS_TARGET.to_string(),
    )];
    let patched = morphic::patch_kv3_resource_strings_adding(&graph, &edits)
        .context("patching graph with out-of-list clip")?;

    let check = morphic::decode_kv3_resource(&patched)?;
    match check.get("m_resources") {
        Some(Value::Array(rs)) if matches!(rs.get(from_idx), Some(Value::String(s)) if s == ABRAMS_TARGET) =>
        {
            println!(
                "[C] redirect verified: m_resources[{from_idx}] -> {ABRAMS_TARGET} (out-of-list)"
            );
        }
        _ => bail!("patched graph did not verify"),
    }

    let out = format!("{out_dir}/pak_expC2_abrams_knockdown_idle_dir.vpk");
    vpkmerge_core::pack(&[(ABRAMS_GRAPH, patched.as_slice())], &out)?;
    println!("[C] -> {out}");
    Ok(())
}

fn experiment_d(pak: &str, out_dir: &str) -> Result<()> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in ABRAMS_RUNS {
        let clip = vpkmerge_core::read_vpk_entry(pak, entry)?;
        let dur = read_duration(&clip).with_context(|| format!("reading duration of {entry}"))?;
        let doubled = dur * 2.0;
        let edits = vec![(vec![Seg::Key("m_flDuration".into())], doubled)];
        let patched = morphic::patch_kv3_resource_doubles(&clip, &edits)
            .with_context(|| format!("patching duration of {entry}"))?;
        let new_dur = read_duration(&patched)?;
        println!("[D] {entry}: duration {dur:.3}s -> {new_dur:.3}s");
        if (new_dur - doubled).abs() > 1e-9 {
            bail!("duration patch did not stick");
        }
        files.push((entry.to_string(), patched));
    }

    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(e, b)| (e.as_str(), b.as_slice()))
        .collect();
    let out = format!("{out_dir}/pak_expD_abrams_slowmo_run_dir.vpk");
    vpkmerge_core::pack(&refs, &out)?;
    println!("[D] -> {out}");
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    experiment_c(&pak, &out_dir)?;
    experiment_d(&pak, &out_dir)?;
    Ok(())
}
