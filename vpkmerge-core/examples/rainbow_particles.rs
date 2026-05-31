// Aggressive particle rainbow probe via in-place scalar patching.
//
// Reads every .vpcf_c under the given prefixes, turns existing gradient stops
// into high-saturation rainbow ramps, gives non-gradient color fields varied
// bright hues, and packs an addon VPK. This preserves the compiled KV3 resource
// framing by only patching existing numeric color channels.
//
// usage:
//   cargo run -p vpkmerge-core --example rainbow_particles -- \
//     <base_dir.vpk> <out_dir.vpk> <prefix>...
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

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [i64; 3] {
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
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
    [
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as i64,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as i64,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as i64,
    ]
}

fn as_color(v: &Value) -> Option<[i64; 3]> {
    let Value::Array(items) = v else { return None };
    if items.len() != 3 && items.len() != 4 {
        return None;
    }
    let mut ch = [0i64; 3];
    for (i, it) in items.iter().enumerate() {
        let n = match it {
            Value::Int(n) if (0..=255).contains(n) => *n,
            Value::UInt(u) if *u <= 255 => i64::try_from(*u).ok()?,
            _ => return None,
        };
        if i < 3 {
            ch[i] = n;
        }
    }
    Some(ch)
}

fn hash_hue(s: &str) -> f64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    (h % 360) as f64
}

fn path_label(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            Seg::Key(k) => {
                out.push('/');
                out.push_str(k);
            }
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

fn rainbow_gradient_stop(
    rgb: [i64; 3],
    entry: &str,
    path: &[Seg],
    index: usize,
    count: usize,
) -> [i64; 3] {
    let (_, _, v) = rgb_to_hsv(
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    let hue = if count <= 1 {
        hash_hue(&format!("{entry}{}", path_label(path)))
    } else {
        index as f64 * 360.0 / count as f64
    };
    let val = if v < 0.02 { 0.45 } else { v.max(0.92) };
    hsv_to_rgb(hue, 1.0, val)
}

fn rainbow_color_field(rgb: [i64; 3], entry: &str, path: &[Seg]) -> [i64; 3] {
    let (_, _, v) = rgb_to_hsv(
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    if v < 0.02 {
        return rgb;
    }

    let label = path_label(path);
    let mut hue = hash_hue(&format!("{entry}{label}"));
    if label.ends_with("/m_ColorMax") {
        hue = (hue + 180.0).rem_euclid(360.0);
    } else if label.ends_with("/m_ColorFade") {
        hue = (hue + 90.0).rem_euclid(360.0);
    }
    hsv_to_rgb(hue, 1.0, v.max(0.82))
}

fn path_is_stops(path: &[Seg]) -> bool {
    matches!(path.last(), Some(Seg::Key(k)) if k == "m_Stops")
}

fn collect_edits(
    entry: &str,
    v: &Value,
    path: &mut Vec<Seg>,
    colorish: bool,
    gradient_stop: Option<(usize, usize)>,
    edits: &mut Vec<(Vec<Seg>, i64)>,
    gradient_fields: &mut usize,
    color_fields: &mut usize,
) {
    if colorish {
        if let Some(rgb) = as_color(v) {
            let new = if let Some((i, n)) = gradient_stop {
                *gradient_fields += 1;
                rainbow_gradient_stop(rgb, entry, path, i, n)
            } else {
                *color_fields += 1;
                rainbow_color_field(rgb, entry, path)
            };
            for (i, &nv) in new.iter().enumerate() {
                if nv != rgb[i] {
                    let mut p = path.clone();
                    p.push(Seg::Index(i));
                    edits.push((p, nv));
                }
            }
            return;
        }
    }

    match v {
        Value::Object(pairs) => {
            for (k, child) in pairs {
                let kl = k.to_lowercase();
                let c = kl.contains("color") || kl.contains("tint");
                path.push(Seg::Key(k.clone()));
                collect_edits(
                    entry,
                    child,
                    path,
                    c,
                    gradient_stop,
                    edits,
                    gradient_fields,
                    color_fields,
                );
                path.pop();
            }
        }
        Value::Array(items) => {
            let stops = path_is_stops(path);
            let len = items.len();
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                let child_gradient = if stops { Some((i, len)) } else { gradient_stop };
                collect_edits(
                    entry,
                    item,
                    path,
                    false,
                    child_gradient,
                    edits,
                    gradient_fields,
                    color_fields,
                );
                path.pop();
            }
        }
        _ => {}
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("base_dir.vpk");
    let out = args.next().expect("out_dir.vpk");
    let prefixes: Vec<String> = args.collect();
    anyhow::ensure!(!prefixes.is_empty(), "give at least one path prefix");

    let vpk = valve_pak::open(&base)?;
    let mut entries: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vpcf_c") && prefixes.iter().any(|pre| p.starts_with(pre)))
        .cloned()
        .collect();
    entries.sort();

    let mut packed = Vec::new();
    let mut patched = 0usize;
    let mut no_color = 0usize;
    let mut patch_err = 0usize;
    let mut gradient_fields = 0usize;
    let mut color_fields = 0usize;
    for entry in &entries {
        let bytes = vpk.get_file(entry)?.read_all()?;
        let value = morphic::decode_kv3_resource(&bytes)?;
        let mut edits = Vec::new();
        let mut file_gradient_fields = 0usize;
        let mut file_color_fields = 0usize;
        collect_edits(
            entry,
            &value,
            &mut Vec::new(),
            false,
            None,
            &mut edits,
            &mut file_gradient_fields,
            &mut file_color_fields,
        );
        if edits.is_empty() {
            no_color += 1;
            continue;
        }
        match morphic::patch_kv3_resource_scalars(&bytes, &edits) {
            Ok(new_bytes) => {
                gradient_fields += file_gradient_fields;
                color_fields += file_color_fields;
                packed.push((entry.clone(), new_bytes));
                patched += 1;
            }
            Err(e) => {
                patch_err += 1;
                eprintln!("skip {entry}: {e}");
            }
        }
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {patched} patched, {no_color} no-color, {patch_err} patch-error, {gradient_fields} gradient fields, {color_fields} other color fields"
    );
    Ok(())
}
