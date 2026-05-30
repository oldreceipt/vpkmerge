// Print color-named integer arrays of a .vpcf_c (to verify a recolor landed).
// usage: cargo run --example readcolor -- <vpk> <entry>
use morphic::kv3::Value;

fn is_color(v: &Value) -> Option<Vec<i64>> {
    let Value::Array(items) = v else { return None };
    if items.len() != 3 && items.len() != 4 {
        return None;
    }
    let mut out = Vec::new();
    for it in items {
        match it {
            Value::Int(n) if (0..=255).contains(n) => out.push(*n),
            Value::UInt(u) if *u <= 255 => out.push(*u as i64),
            _ => return None,
        }
    }
    Some(out)
}

fn walk(v: &Value, path: &str, colorish: bool, out: &mut Vec<(String, Vec<i64>)>) {
    if colorish {
        if let Some(c) = is_color(v) {
            out.push((path.to_string(), c));
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

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(a.next().expect("vpk"))?;
    let entry = a.next().expect("entry");
    let mut f = vpk.get_file(&entry).expect("entry");
    let value = morphic::decode_kv3_resource(&f.read_all()?)?;
    let mut out = Vec::new();
    walk(&value, "", false, &mut out);
    for (p, c) in &out {
        println!("  {p} = {c:?}");
    }
    println!("({} color fields)", out.len());
    Ok(())
}
