// Compare a mod VPK's particle set against the base game to find which .vpcf_c
// were actually edited, and characterize the recolor (changed color fields + hue).
// usage: cargo run --example unicorn_scan -- <base.vpk> <mod.vpk> [report.txt]
use morphic::kv3::Value;
use std::collections::BTreeMap;
use std::io::Write;

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
                walk(c, &format!("{path}/{k}"), kl.contains("color") || kl.contains("tint"), out);
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

fn hue(rgb: [f64; 3]) -> Option<f64> {
    let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d < 1.0 {
        return None; // neutral (white/gray/black), no meaningful hue
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

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let base = valve_pak::open(a.next().expect("base vpk"))?;
    let modv = valve_pak::open(a.next().expect("mod vpk"))?;
    let report_path = a.next();

    let mut paths: Vec<String> = modv
        .file_paths()
        .filter(|p| p.ends_with(".vpcf_c"))
        .cloned()
        .collect();
    paths.sort();

    let mut diff_lines: Vec<String> = Vec::new();
    let mut all_changed_hues: Vec<f64> = Vec::new();
    let (mut same, mut newf, mut diff, mut color_changed) = (0u32, 0u32, 0u32, 0u32);

    for p in &paths {
        let mb = raw(&modv, p);
        let bb = raw(&base, p);
        match (bb, mb) {
            (_, None) => continue, // path came from mod listing; mod read should not fail
            (None, Some(_)) => {
                newf += 1;
                diff_lines.push(format!("NEW   {p}"));
            }
            (Some(b), Some(m)) if b == m => same += 1,
            (Some(_), Some(_)) => {
                diff += 1;
                // decode both, find changed color fields
                let bc = colors(&base, p).unwrap_or_default();
                let mc = colors(&modv, p).unwrap_or_default();
                let mut changed = 0u32;
                let mut file_hues: Vec<f64> = Vec::new();
                for (k, bv) in &bc {
                    if let Some(mv) = mc.get(k) {
                        if bv.iter().zip(mv).any(|(x, y)| (x - y).abs() > 0.5) {
                            changed += 1;
                            if let Some(h) = hue(*mv) {
                                file_hues.push(h);
                                all_changed_hues.push(h);
                            }
                        }
                    }
                }
                if changed > 0 {
                    color_changed += 1;
                }
                let avg = if file_hues.is_empty() {
                    "none".to_string()
                } else {
                    format!("{:.0}", file_hues.iter().sum::<f64>() / file_hues.len() as f64)
                };
                let name = p.rsplit('/').next().unwrap_or(p);
                diff_lines.push(format!("DIFF  {name}  colors_changed={changed} hue={avg}"));
            }
        }
    }

    let mean = if all_changed_hues.is_empty() {
        f64::NAN
    } else {
        all_changed_hues.iter().sum::<f64>() / all_changed_hues.len() as f64
    };

    let mut buf = String::new();
    buf.push_str(&format!(
        "vpcf total={} | identical-to-base={} | edited(diff)={} | of-those-with-color-change={} | new={}\n",
        paths.len(), same, diff, color_changed, newf
    ));
    buf.push_str(&format!(
        "mean hue across {} changed color fields = {:.0} deg\n\n",
        all_changed_hues.len(), mean
    ));
    diff_lines.sort();
    for l in &diff_lines {
        buf.push_str(l);
        buf.push('\n');
    }

    if let Some(rp) = report_path {
        let mut f = std::fs::File::create(&rp)?;
        f.write_all(buf.as_bytes())?;
        eprintln!("wrote report to {rp} ({} diff lines)", diff_lines.len());
    } else {
        print!("{buf}");
    }
    Ok(())
}
