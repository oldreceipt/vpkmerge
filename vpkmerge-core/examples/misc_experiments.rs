//! First in-game tests for four untested format routes plus an
//! unused-content unlock, bundled for one launch. See
//! docs/customization-frontier.md for the research behind each.
//!
//! H: music stinger remap (scripts/music_data.vdata_c). Hero-death stinger
//!    repointed at the kill-streak stinger soundevent: dying sounds wrong.
//! I: post-process patch (postprocessing/basepostprocess_deadlock.vpost_c).
//!    Tonemap exposure bias dropped: whole screen reads darker/moodier.
//! J: unused-content unlock: Yamato's unshipped Christmas body material
//!    packed over her default body material.
//! K: soundevents pitch (soundevents/hero/yamato.vsndevts_c). Weapon events
//!    pitched to 0.5: deep slow gun. Closes the spike doc's pending in-game
//!    verification of set_event_field.
//! L: disco light styles (scripts/light_styles.vdata_c). Every dimmer spline
//!    becomes a hard strobe. Signal depends on the map using styled lights;
//!    a null here is acceptable.
//!
//! Usage: cargo run --release -p vpkmerge-core --example misc_experiments -- \
//!     <pak01_dir.vpk> <out_dir>

use anyhow::{bail, Context, Result};
use morphic::kv3::{Seg, Value};

const MUSIC_DATA: &str = "scripts/music_data.vdata_c";
const BASE_VPOST: &str = "postprocessing/basepostprocess_deadlock.vpost_c";
const YAMATO_BODY_VMAT: &str = "models/heroes_staging/yamato_v2/materials/yamato_shape.vmat_c";
const YAMATO_XMAS_VMAT: &str = "models/heroes_staging/yamato_v2/materials/yamato_shape_xmas.vmat_c";
const YAMATO_SOUNDS: &str = "soundevents/hero/yamato.vsndevts_c";
const LIGHT_STYLES: &str = "scripts/light_styles.vdata_c";

/// Depth-first search for an object member named `key`; returns the seg path.
fn find_key(v: &Value, key: &str, path: &mut Vec<Seg>) -> Option<Vec<Seg>> {
    match v {
        Value::Object(members) => {
            for (k, child) in members {
                if k == key {
                    let mut found = path.clone();
                    found.push(Seg::Key(k.clone()));
                    return Some(found);
                }
                path.push(Seg::Key(k.clone()));
                if let Some(found) = find_key(child, key, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                if let Some(found) = find_key(child, key, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }
        _ => None,
    }
}

fn get_at<'a>(mut v: &'a Value, path: &[Seg]) -> Option<&'a Value> {
    for seg in path {
        v = match seg {
            Seg::Key(k) => v.get(k)?,
            Seg::Index(i) => match v {
                Value::Array(items) => items.get(*i)?,
                _ => return None,
            },
        };
    }
    Some(v)
}

fn experiment_h(pak: &str, out_dir: &str) -> Result<()> {
    let bytes = vpkmerge_core::read_vpk_entry(pak, MUSIC_DATA)?;
    let root = morphic::decode_kv3_resource(&bytes)?;

    let death = find_key(&root, "EStinger_HeroDeath", &mut Vec::new())
        .context("EStinger_HeroDeath not found")?;
    let donor_state = ["EStinger_KillStreak_10", "EStinger_Respawn"]
        .iter()
        .find_map(|s| find_key(&root, s, &mut Vec::new()))
        .context("no donor stinger found")?;

    let mut death_event_path = death;
    death_event_path.push(Seg::Key("m_MusicStateDefault".into()));
    death_event_path.push(Seg::Key("m_strSoundEvent".into()));
    let mut donor_event_path = donor_state;
    donor_event_path.push(Seg::Key("m_MusicStateDefault".into()));
    donor_event_path.push(Seg::Key("m_strSoundEvent".into()));

    let old = match get_at(&root, &death_event_path) {
        Some(Value::String(s)) => s.clone(),
        other => bail!("death stinger soundevent not a string: {other:?}"),
    };
    let new = match get_at(&root, &donor_event_path) {
        Some(Value::String(s)) => s.clone(),
        other => bail!("donor stinger soundevent not a string: {other:?}"),
    };

    let patched =
        morphic::patch_kv3_resource_strings_adding(&bytes, &[(death_event_path, new.clone())])?;
    println!("[H] hero-death stinger: {old} -> {new}");

    let out = format!("{out_dir}/pak_expH_death_stinger_dir.vpk");
    vpkmerge_core::pack(&[(MUSIC_DATA, patched.as_slice())], &out)?;
    println!("[H] -> {out}");
    Ok(())
}

fn experiment_i(pak: &str, out_dir: &str) -> Result<()> {
    let bytes = vpkmerge_core::read_vpk_entry(pak, BASE_VPOST)?;
    let root = morphic::decode_kv3_resource(&bytes)?;
    let path = vec![
        Seg::Key("m_toneMapParams".into()),
        Seg::Key("m_flExposureBias".into()),
    ];
    let old = match get_at(&root, &path) {
        Some(Value::Double(d)) => *d,
        other => bail!("exposure bias not a double: {other:?}"),
    };
    let new = -1.5;
    let patched = morphic::patch_kv3_resource_doubles(&bytes, &[(path, new)])?;
    println!("[I] base exposure bias: {old:.3} -> {new:.3}");

    let out = format!("{out_dir}/pak_expI_dark_world_dir.vpk");
    vpkmerge_core::pack(&[(BASE_VPOST, patched.as_slice())], &out)?;
    println!("[I] -> {out}");
    Ok(())
}

fn experiment_j(pak: &str, out_dir: &str) -> Result<()> {
    let xmas = vpkmerge_core::read_vpk_entry(pak, YAMATO_XMAS_VMAT)?;
    let out = format!("{out_dir}/pak_expJ_yamato_xmas_dir.vpk");
    vpkmerge_core::pack(&[(YAMATO_BODY_VMAT, xmas.as_slice())], &out)?;
    println!("[J] yamato xmas material over default body -> {out}");
    Ok(())
}

fn experiment_k(pak: &str, out_dir: &str) -> Result<()> {
    let mut se = vpkmerge_core::soundevents::SoundEvents::from_vpk(pak, YAMATO_SOUNDS)?;
    let weapon_events: Vec<String> = se
        .event_names()
        .iter()
        .filter(|n| n.contains("Wpn"))
        .map(|n| (*n).to_string())
        .collect();
    if weapon_events.is_empty() {
        bail!("no Wpn events in {YAMATO_SOUNDS}");
    }
    let mut took = 0usize;
    for ev in &weapon_events {
        if se.set_event_field(ev, "pitch", 0.5) {
            took += 1;
        }
    }
    println!(
        "[K] pitch 0.5 applied to {took}/{} Wpn events (e.g. {})",
        weapon_events.len(),
        weapon_events.first().unwrap()
    );
    if took == 0 {
        bail!("no event accepted a pitch field");
    }
    let encoded = se.encode()?;
    let out = format!("{out_dir}/pak_expK_yamato_deep_gun_dir.vpk");
    vpkmerge_core::pack(&[(YAMATO_SOUNDS, encoded.as_slice())], &out)?;
    println!("[K] -> {out}");
    Ok(())
}

fn experiment_l(pak: &str, out_dir: &str) -> Result<()> {
    let bytes = vpkmerge_core::read_vpk_entry(pak, LIGHT_STYLES)?;
    let root = morphic::decode_kv3_resource(&bytes)?;
    let Value::Object(styles) = &root else {
        bail!("light_styles root is not an object");
    };

    let mut edits: Vec<(Vec<Seg>, f64)> = Vec::new();
    let mut styled = 0usize;
    for (name, style) in styles {
        let Some(Value::Array(points)) = style.get("dimmer").and_then(|d| d.get("m_spline")) else {
            continue;
        };
        styled += 1;
        for (i, p) in points.iter().enumerate() {
            if matches!(p.get("y"), Some(Value::Double(_) | Value::Int(_))) {
                edits.push((
                    vec![
                        Seg::Key(name.clone()),
                        Seg::Key("dimmer".into()),
                        Seg::Key("m_spline".into()),
                        Seg::Index(i),
                        Seg::Key("y".into()),
                    ],
                    if i % 2 == 0 { 2.0 } else { 0.0 },
                ));
            }
        }
    }
    if edits.is_empty() {
        bail!("no spline points found to strobe");
    }

    // On-disk storage per value may be double, float32, or int (KV3 packs
    // whole-number floats as ints); the byte-faithful patchers are strict
    // about type, so patch one edit at a time with type fallbacks.
    let mut patched = bytes.clone();
    let mut took = 0usize;
    for (path, val) in edits.iter().cloned() {
        let attempt = morphic::patch_kv3_resource_doubles(&patched, &[(path.clone(), val)])
            .or_else(|_| {
                #[allow(clippy::cast_possible_truncation)]
                morphic::patch_kv3_resource_floats(&patched, &[(path.clone(), val as f32)])
            })
            .or_else(|_| {
                #[allow(clippy::cast_possible_truncation)]
                morphic::patch_kv3_resource_scalars(&patched, &[(path, val as i64)])
            });
        if let Ok(next) = attempt {
            patched = next;
            took += 1;
        }
    }
    if took == 0 {
        bail!("no spline point accepted a patch");
    }
    println!(
        "[L] strobed {styled} styles ({took}/{} spline points)",
        edits.len()
    );

    let out = format!("{out_dir}/pak_expL_disco_lights_dir.vpk");
    vpkmerge_core::pack(&[(LIGHT_STYLES, patched.as_slice())], &out)?;
    println!("[L] -> {out}");
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let pak = args.next().context("missing arg: path to pak01_dir.vpk")?;
    let out_dir = args.next().context("missing arg: output directory")?;

    experiment_h(&pak, &out_dir)?;
    experiment_i(&pak, &out_dir)?;
    experiment_j(&pak, &out_dir)?;
    experiment_k(&pak, &out_dir)?;
    experiment_l(&pak, &out_dir)?;
    Ok(())
}
