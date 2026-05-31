use morphic::kv3::Value;
fn show(v: &Value, depth: usize) {
    match v {
        Value::Object(o) => {
            for (k, val) in o {
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        println!("{}{}/", "  ".repeat(depth), k);
                        show(val, depth + 1);
                    }
                    _ => println!("{}{} = {:?}", "  ".repeat(depth), k, val),
                }
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                println!("{}[{}]", "  ".repeat(depth), i);
                show(val, depth + 1);
            }
        }
        _ => {}
    }
}
fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(o) => {
            for (k, val) in o {
                if k == key {
                    return Some(val);
                }
                if let Some(f) = find(val, key) {
                    return Some(f);
                }
            }
            None
        }
        Value::Array(a) => {
            for val in a {
                if let Some(f) = find(val, key) {
                    return Some(f);
                }
            }
            None
        }
        _ => None,
    }
}
fn main() -> anyhow::Result<()> {
    let pak = std::env::args().nth(1).unwrap();
    let e = std::env::args().nth(2).unwrap();
    let key = std::env::args().nth(3).unwrap();
    let b = vpkmerge_core::read_vpk_entry(&pak, &e)?;
    let v = morphic::decode_kv3_resource(&b)?;
    if let Some(sub) = find(&v, &key) {
        println!("=== {key} subtree ===");
        show(sub, 1);
    } else {
        println!("{key} not found");
    }
    Ok(())
}
