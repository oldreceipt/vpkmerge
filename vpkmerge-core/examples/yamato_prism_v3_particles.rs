// Yamato Prism V3 particle pass.
//
// This is still an in-place scalar patcher: it only changes existing Color32
// numeric fields in compiled .vpcf_c files. The goal is a cleaner spectral look
// than the aggressive rainbow probe:
//   - ability-family hue bands, so the kit reads coherently
//   - stronger value floors on glow/light/beam/core fields
//   - wider min/max/fade offsets for existing random/interpolated color fields
//
// usage:
//   cargo run -p vpkmerge-core --example yamato_prism_v3_particles -- \
//     <base_dir.vpk> <out_dir.vpk> <prefix>...
use morphic::kv3::{Seg, Value};

#[derive(Debug, Clone, Copy)]
struct Theme {
    base: f64,
    span: f64,
    jitter: f64,
}

#[derive(Default)]
struct PatchStats {
    gradient_fields: usize,
    color_fields: usize,
    boosted_fields: usize,
    random_range_fields: usize,
}

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

fn hash01(s: &str) -> f64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h as f64 / u64::MAX as f64
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

fn theme_for(entry: &str) -> Theme {
    let e = entry.to_ascii_lowercase();
    if e.contains("infinity_slash") {
        Theme {
            base: 350.0,
            span: 320.0,
            jitter: 22.0,
        }
    } else if e.contains("shadow_form") || e.contains("shadow_redemption") {
        Theme {
            base: 250.0,
            span: 135.0,
            jitter: 18.0,
        }
    } else if e.contains("power_slash") {
        Theme {
            base: 285.0,
            span: 145.0,
            jitter: 18.0,
        }
    } else if e.contains("blade_dash") {
        Theme {
            base: 135.0,
            span: 155.0,
            jitter: 18.0,
        }
    } else if e.contains("flying_strike") {
        Theme {
            base: 185.0,
            span: 155.0,
            jitter: 20.0,
        }
    } else if e.contains("decimate") || e.contains("crimson_slash") {
        Theme {
            base: 325.0,
            span: 115.0,
            jitter: 14.0,
        }
    } else if e.contains("explosive_dart") || e.contains("flash_bomb") {
        Theme {
            base: 45.0,
            span: 185.0,
            jitter: 20.0,
        }
    } else if e.contains("nightmare") {
        Theme {
            base: 245.0,
            span: 135.0,
            jitter: 18.0,
        }
    } else if e.contains("counter") || e.contains("chaff") {
        Theme {
            base: 95.0,
            span: 170.0,
            jitter: 20.0,
        }
    } else if e.contains("weapon_fx")
        || e.contains("tracer")
        || e.contains("blade_glow")
        || e.contains("blade_sweep")
    {
        Theme {
            base: 5.0,
            span: 330.0,
            jitter: 24.0,
        }
    } else if e.contains("blink") {
        Theme {
            base: 195.0,
            span: 175.0,
            jitter: 20.0,
        }
    } else {
        Theme {
            base: 0.0,
            span: 300.0,
            jitter: 24.0,
        }
    }
}

fn spectral_path(label: &str) -> bool {
    [
        "glow", "light", "beam", "core", "flash", "ring", "symbol", "energy", "magic", "trail",
        "arc", "rope", "slash", "sweep", "tracer", "streak", "pulse", "endcap",
    ]
    .iter()
    .any(|needle| label.contains(needle))
}

fn subdued_path(label: &str) -> bool {
    ["smoke", "dust", "debris", "darkness", "fog", "gas"]
        .iter()
        .any(|needle| label.contains(needle))
}

fn hue_at(theme: Theme, entry: &str, label: &str, t: f64) -> f64 {
    let jitter = (hash01(&format!("{entry}{label}")) - 0.5) * 2.0 * theme.jitter;
    theme.base + theme.span * t + jitter
}

fn value_floor(source_v: f64, label: &str, gradient: bool) -> (f64, bool) {
    if source_v < 0.02 {
        return if gradient && spectral_path(label) {
            (0.30, true)
        } else {
            (source_v, false)
        };
    }

    if subdued_path(label) {
        (source_v.max(0.48), false)
    } else if spectral_path(label) {
        (source_v.max(0.96), source_v < 0.96)
    } else if gradient {
        (source_v.max(0.86), source_v < 0.86)
    } else {
        (source_v.max(0.78), source_v < 0.78)
    }
}

fn saturation_for(label: &str) -> f64 {
    if subdued_path(label) {
        0.82
    } else {
        1.0
    }
}

fn prism_gradient_stop(
    rgb: [i64; 3],
    entry: &str,
    path: &[Seg],
    index: usize,
    count: usize,
    position: Option<f64>,
    stats: &mut PatchStats,
) -> [i64; 3] {
    stats.gradient_fields += 1;
    let (_, _, v) = rgb_to_hsv(
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    let label = path_label(path).to_ascii_lowercase();
    let theme = theme_for(entry);
    let t = if count <= 1 {
        hash01(&format!("{entry}{label}"))
    } else if count == 2 {
        [0.10, 0.82][index.min(1)]
    } else if let Some(position) = position {
        position.clamp(0.0, 1.0)
    } else {
        index as f64 / (count - 1) as f64
    };
    let hue = hue_at(theme, entry, &label, t);
    let (val, boosted) = value_floor(v, &label, true);
    if boosted {
        stats.boosted_fields += 1;
    }
    hsv_to_rgb(hue, saturation_for(&label), val)
}

fn prism_color_field(rgb: [i64; 3], entry: &str, path: &[Seg], stats: &mut PatchStats) -> [i64; 3] {
    stats.color_fields += 1;
    let (_, _, v) = rgb_to_hsv(
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    if v < 0.02 {
        return rgb;
    }

    let label = path_label(path).to_ascii_lowercase();
    let theme = theme_for(entry);
    let base_t = hash01(&format!("{entry}{label}"));
    let t = if label.ends_with("/m_colormin") {
        stats.random_range_fields += 1;
        0.02 + base_t * 0.18
    } else if label.ends_with("/m_colormax") {
        stats.random_range_fields += 1;
        0.70 + base_t * 0.28
    } else if label.ends_with("/m_colorfade") {
        stats.random_range_fields += 1;
        0.40 + base_t * 0.35
    } else {
        base_t
    };
    let hue = hue_at(theme, entry, &label, t);
    let (val, boosted) = value_floor(v, &label, false);
    if boosted {
        stats.boosted_fields += 1;
    }
    hsv_to_rgb(hue, saturation_for(&label), val)
}

fn path_is_stops(path: &[Seg]) -> bool {
    matches!(path.last(), Some(Seg::Key(k)) if k == "m_Stops")
}

fn collect_edits(
    entry: &str,
    v: &Value,
    path: &mut Vec<Seg>,
    colorish: bool,
    gradient_stop: Option<(usize, usize, Option<f64>)>,
    edits: &mut Vec<(Vec<Seg>, i64)>,
    stats: &mut PatchStats,
) {
    if colorish {
        if let Some(rgb) = as_color(v) {
            let new = if let Some((i, n, position)) = gradient_stop {
                prism_gradient_stop(rgb, entry, path, i, n, position, stats)
            } else {
                prism_color_field(rgb, entry, path, stats)
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
                collect_edits(entry, child, path, c, gradient_stop, edits, stats);
                path.pop();
            }
        }
        Value::Array(items) => {
            let stops = path_is_stops(path);
            let len = items.len();
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                let child_gradient = if stops {
                    let position = item.get("m_flPosition").and_then(Value::as_f64);
                    Some((i, len, position))
                } else {
                    gradient_stop
                };
                collect_edits(entry, item, path, false, child_gradient, edits, stats);
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
    let mut stats = PatchStats::default();
    for entry in &entries {
        let bytes = vpk.get_file(entry)?.read_all()?;
        let value = morphic::decode_kv3_resource(&bytes)?;
        let mut edits = Vec::new();
        let mut file_stats = PatchStats::default();
        collect_edits(
            entry,
            &value,
            &mut Vec::new(),
            false,
            None,
            &mut edits,
            &mut file_stats,
        );
        if edits.is_empty() {
            no_color += 1;
            continue;
        }
        match morphic::patch_kv3_resource_scalars(&bytes, &edits) {
            Ok(new_bytes) => {
                stats.gradient_fields += file_stats.gradient_fields;
                stats.color_fields += file_stats.color_fields;
                stats.boosted_fields += file_stats.boosted_fields;
                stats.random_range_fields += file_stats.random_range_fields;
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
        "wrote {out}: {patched} patched, {no_color} no-color, {patch_err} patch-error, {} gradient fields, {} other color fields, {} boosted, {} random/fade range fields",
        stats.gradient_fields, stats.color_fields, stats.boosted_fields, stats.random_range_fields
    );
    Ok(())
}
