// Particle recolor via in-place scalar patching (preserves KV3 v5 framing,
// value flags, and typed-array tags, unlike a full re-encode). Reads every
// .vpcf_c under the given prefixes from a base VPK, retints color-named integer
// arrays to a target hue (preserving each color's saturation/value), and packs
// an addon VPK that overrides the base particles in place.
//
// usage: cargo run --example recolor_particles -- <base.vpk> <out_dir.vpk> <hue_deg> <prefix>...
use morphic::kv3::{Seg, Value};

fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r1 + m, g1 + m, b1 + m)
}

/// If `v` is a numeric array of length 3-4 in Color32 range, return its RGB ints.
fn as_color(v: &Value) -> Option<[i64; 3]> {
    let Value::Array(items) = v else { return None };
    if items.len() != 3 && items.len() != 4 {
        return None;
    }
    let mut ch = [0i64; 3];
    for (i, it) in items.iter().enumerate() {
        let n = match it {
            Value::Int(n) if (0..=255).contains(n) => *n,
            Value::UInt(u) if *u <= 255 => *u as i64,
            _ => return None,
        };
        if i < 3 {
            ch[i] = n;
        }
    }
    Some(ch)
}

fn recolored(rgb: [i64; 3], hue: f64) -> [i64; 3] {
    let (_, s, val) = rgb_to_hsv(
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    let (r, g, b) = hsv_to_rgb(hue, s, val);
    [
        (r * 255.0).round().clamp(0.0, 255.0) as i64,
        (g * 255.0).round().clamp(0.0, 255.0) as i64,
        (b * 255.0).round().clamp(0.0, 255.0) as i64,
    ]
}

/// Walk the tree, building scalar edits for color channels. `path` is the path
/// to the current value; `colorish` is true when reached via a color/tint key.
fn collect_edits(
    v: &Value,
    path: &mut Vec<Seg>,
    colorish: bool,
    hue: f64,
    edits: &mut Vec<(Vec<Seg>, i64)>,
) {
    if colorish {
        if let Some(rgb) = as_color(v) {
            let new = recolored(rgb, hue);
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
                collect_edits(child, path, c, hue, edits);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                collect_edits(item, path, false, hue, edits);
                path.pop();
            }
        }
        _ => {}
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("base vpk");
    let out = args.next().expect("out_dir.vpk");
    let hue: f64 = args.next().expect("hue degrees").parse()?;
    let prefixes: Vec<String> = args.collect();
    anyhow::ensure!(!prefixes.is_empty(), "give at least one path prefix");

    let vpk = valve_pak::open(&base)?;
    let entries: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vpcf_c") && prefixes.iter().any(|pre| p.starts_with(pre)))
        .cloned()
        .collect();

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let (mut patched, mut skipped_nocolor, mut skipped_err) = (0usize, 0usize, 0usize);
    for entry in &entries {
        let mut f = vpk.get_file(entry).expect("entry");
        let bytes = f.read_all()?;
        let value = morphic::decode_kv3_resource(&bytes)?;
        let mut edits = Vec::new();
        collect_edits(&value, &mut Vec::new(), false, hue, &mut edits);
        if edits.is_empty() {
            skipped_nocolor += 1;
            continue;
        }
        match morphic::patch_kv3_resource_scalars(&bytes, &edits) {
            Ok(new_bytes) => {
                packed.push((entry.clone(), new_bytes));
                patched += 1;
            }
            Err(e) => {
                skipped_err += 1;
                eprintln!("  skip {entry}: {e}");
            }
        }
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {patched} patched, {skipped_nocolor} no-color, {skipped_err} patch-error (of {} files), hue {hue} deg",
        entries.len()
    );
    Ok(())
}
