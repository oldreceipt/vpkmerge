// Identify the COLOR-BEARING textures a hero's bullets + abilities render with,
// i.e. the .vtex_c that carry baked chroma and so won't fully recolor from a
// particle color-param edit alone (they need their own hue shift). Walks
// particle .vpcf_c -> referenced materials (.vmat_c) -> referenced textures
// (.vtex_c), then classifies each texture by mean saturation.
//
// usage: cargo run --example recolor_assets -- <base.vpk> <codename>   (e.g. bookworm)
use morphic::kv3::Value;
use morphic::{ImageData, TextureFormat};
use std::collections::{BTreeMap, BTreeSet};

/// Collect every string leaf in a KV3 tree (resource refs are plain strings once
/// decoded; their flags are dropped but the path content is intact).
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.iter().for_each(|(_, x)| strings(x, out)),
        _ => {}
    }
}

/// Normalize a source-asset ref to its compiled entry path (`.vmat` -> `.vmat_c`).
fn compiled(p: &str) -> String {
    if p.ends_with("_c") {
        p.to_string()
    } else {
        format!("{p}_c")
    }
}

fn refs_with_ext<'a>(strs: &'a [String], ext: &str) -> impl Iterator<Item = String> + 'a {
    let ext = ext.to_string();
    strs.iter()
        .filter(move |s| s.ends_with(&ext) || s.ends_with(&format!("{ext}_c")))
        .map(|s| compiled(s))
}

/// Mean saturation over opaque-ish pixels, or None if undecodable / fully transparent.
fn mean_saturation(bytes: &[u8]) -> Option<(f32, &'static str)> {
    let info = morphic::inspect(bytes).ok()?;
    // Single-channel (BC4) and two-channel normal (BC5) carry no chroma.
    match info.format {
        TextureFormat::Ati1n => return Some((0.0, "BC4 mask")),
        TextureFormat::Ati2n => return Some((0.0, "BC5 normal")),
        _ => {}
    }
    let img = morphic::decode(bytes).ok()?;
    let ImageData::Rgba8(px) = img.data else {
        return Some((-1.0, "HDR/other"));
    };
    let (mut sum, mut n) = (0f64, 0u64);
    for c in px.chunks_exact(4) {
        if c[3] < 16 {
            continue;
        }
        let (max, min) = (
            c[0].max(c[1]).max(c[2]) as f32,
            c[0].min(c[1]).min(c[2]) as f32,
        );
        sum += (if max > 0.0 { (max - min) / max } else { 0.0 }) as f64;
        n += 1;
    }
    if n == 0 {
        None
    } else {
        Some(((sum / n as f64) as f32, "RGBA"))
    }
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let base = a.next().expect("base vpk");
    let cn = a.next().expect("codename, e.g. bookworm");
    let vpk = valve_pak::open(&base)?;

    let groups = [
        ("bullets (weapon_fx)", format!("particles/weapon_fx/{cn}/")),
        ("abilities", format!("particles/abilities/{cn}/")),
    ];

    // texture entry -> set of "group:material" that pulls it in
    let mut tex_users: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut missing_mats: BTreeSet<String> = BTreeSet::new();

    for (label, prefix) in &groups {
        let particles: Vec<String> = vpk
            .file_paths()
            .filter(|p| p.starts_with(prefix) && p.ends_with(".vpcf_c"))
            .cloned()
            .collect();
        let mut mats: BTreeSet<String> = BTreeSet::new();
        for p in &particles {
            let mut f = vpk.get_file(p).unwrap();
            let bytes = f.read_all()?;
            let Ok(v) = morphic::decode_kv3_resource(&bytes) else {
                continue;
            };
            let mut ss = Vec::new();
            strings(&v, &mut ss);
            for m in refs_with_ext(&ss, ".vmat") {
                mats.insert(m);
            }
        }
        for m in &mats {
            let Ok(mut mf) = vpk.get_file(m) else {
                missing_mats.insert(m.clone());
                continue;
            };
            let mbytes = mf.read_all()?;
            let Ok(mv) = morphic::decode_kv3_resource(&mbytes) else {
                continue;
            };
            let mut ms = Vec::new();
            strings(&mv, &mut ms);
            for t in refs_with_ext(&ms, ".vtex") {
                tex_users
                    .entry(t)
                    .or_default()
                    .insert(format!("{label}: {}", m.rsplit('/').next().unwrap()));
            }
        }
    }

    // A texture is a data map (not color) if its name marks it as a
    // normal/roughness/AO/mask/metalness channel, regardless of measured chroma
    // (packed normal maps read as ~0.5 saturation but carry no albedo color).
    let is_datamap = |p: &str| {
        ["normal", "rough", "_ao_", "mask", "metal", "selfillummask"]
            .iter()
            .any(|m| p.contains(m))
    };
    // Hero-specific iff the path names the codename; shared defaults/generics
    // (materials/default, materials/particle/{projected,model}) must NOT be
    // recolored or the change bleeds onto other heroes.
    let hero_specific = |p: &str| p.contains(&format!("{cn}"));

    let mut targets: Vec<(String, f32)> = Vec::new(); // hero-specific, color-bearing
    let mut shared_color: Vec<(String, f32)> = Vec::new();
    let mut masks: Vec<String> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for (t, _) in &tex_users {
        let Ok(mut tf) = vpk.get_file(t) else {
            unresolved.push(t.clone());
            continue;
        };
        let tb = tf.read_all()?;
        let colored = !is_datamap(t) && matches!(mean_saturation(&tb), Some((s, _)) if s >= 0.12);
        let sat = mean_saturation(&tb).map(|(s, _)| s).unwrap_or(0.0);
        if !colored {
            masks.push(t.clone());
        } else if hero_specific(t) {
            targets.push((t.clone(), sat));
        } else {
            shared_color.push((t.clone(), sat));
        }
    }
    targets.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    shared_color.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("Paige ({cn}) recolor-asset scan");
    println!(
        "  referenced textures: {}  materials missing from base: {}",
        tex_users.len(),
        missing_mats.len()
    );
    println!(
        "\n== RECOLOR TARGETS (hero-specific, color-bearing) : {} ==",
        targets.len()
    );
    for (t, sat) in &targets {
        println!(
            "  sat {sat:.2}  {t}\n        <- {}",
            tex_users[t].iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    println!(
        "\n== shared color textures (recoloring bleeds to other heroes, AVOID) : {} ==",
        shared_color.len()
    );
    for (t, sat) in &shared_color {
        println!("  sat {sat:.2}  {t}");
    }
    println!(
        "\n== data maps / masks (tinted by particle param, no recolor) : {} ==",
        masks.len()
    );
    for t in masks.iter().take(8) {
        println!("  {t}");
    }
    if masks.len() > 8 {
        println!("  ... +{} more", masks.len() - 8);
    }
    if !unresolved.is_empty() {
        println!(
            "\n== referenced but not in base pak (ship-with-skin?) : {} ==",
            unresolved.len()
        );
        for t in &unresolved {
            println!("  {t}");
        }
    }
    Ok(())
}
