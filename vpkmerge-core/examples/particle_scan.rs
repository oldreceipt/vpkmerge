// Batch-compare several mod VPKs against the base game pak to characterize each
// one's ability-VFX recolor: which entries changed (by type), which `.vpcf_c`
// had color params retinted, and the target hue(s). Generalizes `unicorn_scan`
// (single mod) to a fleet, and auto-detects each mod's hero codename so the
// output labels itself.
//
// usage: cargo run --release --example particle_scan -- <base_dir.vpk> <mod1.vpk> [mod2.vpk ...] [--out <dir>]
//
// With --out, a detailed per-mod report (every DIFF/NEW entry) is written to
// <dir>/scan_<modstem>.txt; stdout always carries the per-mod summary + a final
// combined table.
use morphic::kv3::Value;
use std::collections::BTreeMap;
use std::io::Write;

// ---- color/hue helpers (shared logic with unicorn_scan) --------------------

fn is_color(v: &Value) -> Option<[f64; 3]> {
    let Value::Array(items) = v else { return None };
    if items.len() != 3 && items.len() != 4 {
        return None;
    }
    let mut rgb = [0f64; 3];
    for (i, it) in items.iter().enumerate() {
        let n = match it {
            Value::Int(n) if (0..=255).contains(n) => *n as f64,
            Value::UInt(u) if *u <= 255 => *u as f64,
            _ => return None,
        };
        if i < 3 {
            rgb[i] = n;
        }
    }
    Some(rgb)
}

fn walk(v: &Value, path: &str, colorish: bool, out: &mut BTreeMap<String, [f64; 3]>) {
    if colorish {
        if let Some(c) = is_color(v) {
            out.insert(path.to_string(), c);
            return;
        }
    }
    match v {
        Value::Object(p) => {
            for (k, c) in p {
                let kl = k.to_lowercase();
                walk(
                    c,
                    &format!("{path}/{k}"),
                    kl.contains("color") || kl.contains("tint"),
                    out,
                );
            }
        }
        Value::Array(items) => {
            for (i, it) in items.iter().enumerate() {
                walk(it, &format!("{path}[{i}]"), false, out);
            }
        }
        _ => {}
    }
}

/// HSV hue in degrees, or `None` for a neutral (white/gray/black) color that
/// carries no meaningful hue. A "pink/white" mod shows up as pink hues plus a
/// pile of neutrals; tracking both tells the two apart.
fn hue(rgb: [f64; 3]) -> Option<f64> {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d < 1.0 {
        return None;
    }
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    Some(h.rem_euclid(360.0))
}

fn colors(vpk: &valve_pak::VPK, entry: &str) -> Option<BTreeMap<String, [f64; 3]>> {
    let mut f = vpk.get_file(entry).ok()?;
    let bytes = f.read_all().ok()?;
    let v = morphic::decode_kv3_resource(&bytes).ok()?;
    let mut out = BTreeMap::new();
    walk(&v, "", false, &mut out);
    Some(out)
}

fn raw(vpk: &valve_pak::VPK, entry: &str) -> Option<Vec<u8>> {
    let mut f = vpk.get_file(entry).ok()?;
    f.read_all().ok()
}

fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Circular statistics for hues, which wrap at 360 (so 350 and 10 are 20 apart,
/// not 340). Returns (circular_mean_deg, circular_std_deg, frac_within_30_of_mean).
/// `std` near 0 = one tight target hue; large std / low within-30 fraction = the
/// recolor kept (or introduced) wide hue variation rather than a single color.
fn circular_stats(xs: &[f64]) -> (f64, f64, f64) {
    let n = xs.len() as f64;
    let (mut c, mut s) = (0.0f64, 0.0f64);
    for &h in xs {
        let r = h.to_radians();
        c += r.cos();
        s += r.sin();
    }
    let mean = s.atan2(c).to_degrees().rem_euclid(360.0);
    // 0 = dispersed, 1 = concentrated. Clamp: with all-identical hues float drift
    // can nudge this just above 1.0, which would make the std below NaN.
    let resultant = ((c * c + s * s).sqrt() / n).min(1.0);
    let std = if resultant > 0.0 {
        (-2.0 * resultant.ln()).sqrt().to_degrees()
    } else {
        f64::INFINITY
    };
    let within = xs
        .iter()
        .filter(|&&h| {
            let d = (h - mean).rem_euclid(360.0);
            d.min(360.0 - d) <= 30.0
        })
        .count() as f64
        / n;
    (mean, std, within)
}

/// 12 buckets of 30 deg each, rendered as a one-line bar chart.
fn hue_histogram(xs: &[f64]) -> String {
    let mut buckets = [0u32; 12];
    for &h in xs {
        let b = ((h / 30.0).floor() as usize).min(11);
        buckets[b] += 1;
    }
    let mut s = String::new();
    for (i, &c) in buckets.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let lo = i * 30;
        let hi = lo + 30;
        s.push_str(&format!(
            "    {lo:>3}-{hi:<3} x{c:<4} {}\n",
            "#".repeat(c.min(40) as usize)
        ));
    }
    if s.is_empty() {
        s.push_str("    (none)\n");
    }
    s
}

fn ext_of(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}

/// Most-frequent hero codename across `particles/{abilities,weapon_fx}/<X>/`
/// and `models/heroes*/<X>/` paths, with how many distinct particle dirs each
/// namespace contributed. Returns (label, codename-or-empty).
fn detect_codename(paths: &[String]) -> (String, String) {
    let mut counts: BTreeMap<String, (u32, u32)> = BTreeMap::new(); // codename -> (abilities, weapon_fx)
    for p in paths {
        for (prefix, idx) in [
            ("particles/abilities/", 0usize),
            ("particles/weapon_fx/", 1usize),
        ] {
            if let Some(rest) = p.strip_prefix(prefix) {
                if let Some(code) = rest.split('/').next() {
                    // Skip files that live directly under the prefix (a leaf
                    // `.vpcf_c`, not a per-hero subdir): those have an extension.
                    if code.is_empty() || code.contains('.') {
                        continue;
                    }
                    let e = counts.entry(code.to_string()).or_default();
                    if idx == 0 {
                        e.0 += 1;
                    } else {
                        e.1 += 1;
                    }
                }
            }
        }
    }
    if counts.is_empty() {
        return ("(no hero particle dir found)".to_string(), String::new());
    }
    // pick the codename with the most particle files attributed to it
    let (code, (a, w)) = counts
        .iter()
        .max_by_key(|(_, (a, w))| a + w)
        .map(|(k, v)| (k.clone(), *v))
        .unwrap();
    let others: Vec<&String> = counts.keys().filter(|k| **k != code).collect();
    let mut label = format!("{code} ({a} abilities + {w} weapon_fx .vpcf_c)");
    if !others.is_empty() {
        label.push_str(&format!(" [also touches: {}]", {
            let names: Vec<String> = others.iter().map(|s| (*s).clone()).collect();
            names.join(", ")
        }));
    }
    (label, code)
}

#[derive(Default)]
struct TypeDiff {
    edited: u32,
    new: u32,
    identical: u32,
}

struct ModResult {
    name: String,
    codename: String,
    codename_label: String,
    by_type: BTreeMap<String, TypeDiff>,
    vpcf_edited: u32,
    vpcf_color_changed: u32,
    changed_color_fields: u32,
    neutral_fields: u32, // changed-to-neutral (white/gray) color fields
    hues: Vec<f64>,
    detail: Vec<String>, // per-entry DIFF/NEW lines for the report file
}

fn scan_mod(base: &valve_pak::VPK, mod_path: &str) -> anyhow::Result<ModResult> {
    let modv = valve_pak::open(mod_path)?;
    let name = std::path::Path::new(mod_path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| mod_path.to_string());

    let mut paths: Vec<String> = modv.file_paths().cloned().collect();
    paths.sort();
    let (codename_label, codename) = detect_codename(&paths);

    let mut r = ModResult {
        name,
        codename,
        codename_label,
        by_type: BTreeMap::new(),
        vpcf_edited: 0,
        vpcf_color_changed: 0,
        changed_color_fields: 0,
        neutral_fields: 0,
        hues: Vec::new(),
        detail: Vec::new(),
    };

    for p in &paths {
        let ext = ext_of(p).to_string();
        let mb = raw(&modv, p);
        let bb = raw(base, p);
        let entry = r.by_type.entry(ext.clone()).or_default();
        match (bb, mb) {
            (_, None) => continue, // listed by mod but unreadable; skip
            (None, Some(_)) => {
                entry.new += 1;
                r.detail.push(format!("NEW   {p}"));
            }
            (Some(b), Some(m)) if b == m => {
                entry.identical += 1;
            }
            (Some(_), Some(_)) => {
                entry.edited += 1;
                if ext == "vpcf_c" {
                    r.vpcf_edited += 1;
                    let bc = colors(base, p).unwrap_or_default();
                    let mc = colors(&modv, p).unwrap_or_default();
                    let mut changed = 0u32;
                    let mut file_hues: Vec<f64> = Vec::new();
                    for (k, bv) in &bc {
                        if let Some(mv) = mc.get(k) {
                            if bv.iter().zip(mv).any(|(x, y)| (x - y).abs() > 0.5) {
                                changed += 1;
                                r.changed_color_fields += 1;
                                match hue(*mv) {
                                    Some(h) => {
                                        file_hues.push(h);
                                        r.hues.push(h);
                                    }
                                    None => r.neutral_fields += 1,
                                }
                            }
                        }
                    }
                    if changed > 0 {
                        r.vpcf_color_changed += 1;
                    }
                    let avg = if file_hues.is_empty() {
                        "neutral".to_string()
                    } else {
                        format!(
                            "{:.0}",
                            file_hues.iter().sum::<f64>() / file_hues.len() as f64
                        )
                    };
                    let leaf = p.rsplit('/').next().unwrap_or(p);
                    r.detail
                        .push(format!("DIFF  {leaf}  colors_changed={changed} hue={avg}"));
                } else {
                    let leaf = p.rsplit('/').next().unwrap_or(p);
                    r.detail.push(format!("DIFF  .{ext}  {leaf}"));
                }
            }
        }
    }
    r.detail.sort();
    Ok(r)
}

fn render_summary(r: &ModResult) -> String {
    let mut s = String::new();
    s.push_str(&format!("=== {} ===\n", r.name));
    s.push_str(&format!("hero: {}\n", r.codename_label));
    s.push_str("changed entries by type (edited / new / identical):\n");
    for (ext, d) in &r.by_type {
        if d.edited == 0 && d.new == 0 {
            continue; // only show types that actually changed
        }
        s.push_str(&format!(
            "  .{ext:<10} edited={:<5} new={:<5} identical={}\n",
            d.edited, d.new, d.identical
        ));
    }
    s.push_str(&format!(
        "particle color (.vpcf_c): edited={}  with-color-change={}  changed-fields={} (neutral/white={})\n",
        r.vpcf_edited, r.vpcf_color_changed, r.changed_color_fields, r.neutral_fields
    ));
    if r.hues.is_empty() {
        s.push_str("  hue: (no chromatic color change detected)\n");
    } else {
        let (cmean, cstd, within) = circular_stats(&r.hues);
        s.push_str(&format!(
            "  hue: mean={:.0}  median={:.0}  (n={})\n",
            r.hues.iter().sum::<f64>() / r.hues.len() as f64,
            median(&r.hues),
            r.hues.len()
        ));
        let verdict = if cstd < 15.0 {
            "tight (one target hue)"
        } else if cstd < 35.0 {
            "moderate spread"
        } else {
            "LARGELY VARIABLE"
        };
        s.push_str(&format!(
            "  hue spread: circular_mean={cmean:.0}  circular_std={cstd:.0}deg  within +/-30deg={:.0}%  => {verdict}\n",
            within * 100.0
        ));
        s.push_str("  hue distribution:\n");
        s.push_str(&hue_histogram(&r.hues));
    }
    s
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut out_dir: Option<String> = None;
    if let Some(i) = args.iter().position(|a| a == "--out") {
        out_dir = args.get(i + 1).cloned();
        args.drain(i..=i + 1);
    }
    if args.len() < 2 {
        eprintln!("usage: particle_scan <base_dir.vpk> <mod1.vpk> [mod2.vpk ...] [--out <dir>]");
        std::process::exit(2);
    }
    let base = valve_pak::open(&args[0])?;
    let mods = &args[1..];

    let mut results = Vec::new();
    for m in mods {
        eprintln!("scanning {m} ...");
        match scan_mod(&base, m) {
            Ok(r) => results.push(r),
            Err(e) => eprintln!("  FAILED {m}: {e}"),
        }
    }

    for r in &results {
        print!("{}", render_summary(r));
        println!();
        if let Some(dir) = &out_dir {
            std::fs::create_dir_all(dir)?;
            let stem = std::path::Path::new(&r.name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| r.name.clone());
            let path = format!("{dir}/scan_{stem}.txt");
            let mut f = std::fs::File::create(&path)?;
            f.write_all(render_summary(r).as_bytes())?;
            f.write_all(b"\n--- per-entry detail ---\n")?;
            for l in &r.detail {
                writeln!(f, "{l}")?;
            }
            eprintln!("  report -> {path}");
        }
    }

    // combined table
    println!("=== combined ===");
    println!(
        "{:<34} {:<14} {:>7} {:>7} {:>6} {:>7}",
        "mod", "codename", "vpcf_ed", "colored", "hue~", "median"
    );
    for r in &results {
        let (mean, med) = if r.hues.is_empty() {
            (f64::NAN, f64::NAN)
        } else {
            (
                r.hues.iter().sum::<f64>() / r.hues.len() as f64,
                median(&r.hues),
            )
        };
        let name = if r.name.len() > 33 {
            format!("{}…", &r.name[..32])
        } else {
            r.name.clone()
        };
        println!(
            "{:<34} {:<14} {:>7} {:>7} {:>6.0} {:>7.0}",
            name, r.codename, r.vpcf_edited, r.vpcf_color_changed, mean, med
        );
    }
    Ok(())
}
