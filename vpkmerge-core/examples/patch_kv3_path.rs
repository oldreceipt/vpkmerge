// Byte-faithful KV3 path patch probe for compiled resources.
//
// This deliberately uses morphic's in-place patchers rather than a full KV3
// re-encode, so particle files stay in the v5-style layout the engine accepts.
//
// scan:
//   cargo run -p vpkmerge-core --example patch_kv3_path -- \
//     scan <vpk> <entry> [filter]
//
// patch float:
//   cargo run -p vpkmerge-core --example patch_kv3_path -- \
//     float <vpk> <out_dir.vpk> <entry> <path> <f32-value>
//
// patch double:
//   cargo run -p vpkmerge-core --example patch_kv3_path -- \
//     double <vpk> <out_dir.vpk> <entry> <path> <f64-value>
//
// patch string:
//   cargo run -p vpkmerge-core --example patch_kv3_path -- \
//     string <vpk> <out_dir.vpk> <entry> <path> <existing-string-value>
//
// Path syntax matches the existing diagnostics:
//   /m_Renderers[0]/m_vecTexturesInput[2]/m_Gradient/m_Stops[0]/m_flPosition
use morphic::kv3::{Seg, Value};

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

fn parse_path(path: &str) -> anyhow::Result<Vec<Seg>> {
    anyhow::ensure!(path.starts_with('/'), "path must start with /");
    let mut out = Vec::new();
    for raw in path.split('/').skip(1).filter(|s| !s.is_empty()) {
        let Some((key, mut rest)) = raw.split_once('[') else {
            out.push(Seg::Key(raw.to_string()));
            continue;
        };
        if !key.is_empty() {
            out.push(Seg::Key(key.to_string()));
        }
        loop {
            let (idx, tail) = rest
                .split_once(']')
                .ok_or_else(|| anyhow::anyhow!("unterminated index in {raw:?}"))?;
            out.push(Seg::Index(idx.parse()?));
            let Some(next) = tail.strip_prefix('[') else {
                break;
            };
            rest = next;
        }
    }
    Ok(out)
}

fn walk_scan(v: &Value, path: &mut Vec<Seg>, filter: Option<&str>) {
    let label = path_label(path);
    let show = filter.is_none_or(|f| label.to_ascii_lowercase().contains(f));
    match v {
        Value::String(s) if show => println!("string {label} = {s:?}"),
        Value::Double(d) if show => println!("number {label} = {d}"),
        Value::Int(i) if show => println!("int    {label} = {i}"),
        Value::UInt(u) if show => println!("uint   {label} = {u}"),
        Value::Bool(b) if show => println!("bool   {label} = {b}"),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                walk_scan(item, path, filter);
                path.pop();
            }
        }
        Value::Object(pairs) => {
            for (key, child) in pairs {
                path.push(Seg::Key(key.clone()));
                walk_scan(child, path, filter);
                path.pop();
            }
        }
        _ => {}
    }
}

fn read_entry(vpk_path: &str, entry: &str) -> anyhow::Result<Vec<u8>> {
    let vpk = valve_pak::open(vpk_path)?;
    Ok(vpk.get_file(entry)?.read_all()?)
}

fn pack_one(out: &str, entry: &str, bytes: &[u8]) -> anyhow::Result<()> {
    vpkmerge_core::pack(&[(entry, bytes)], out)?;
    println!("wrote {out}: {entry} ({} bytes)", bytes.len());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("scan|float|string");
    match mode.as_str() {
        "scan" => {
            let vpk = args.next().expect("vpk");
            let entry = args.next().expect("entry");
            let filter = args.next().map(|s| s.to_ascii_lowercase());
            let bytes = read_entry(&vpk, &entry)?;
            let value = morphic::decode_kv3_resource(&bytes)?;
            walk_scan(&value, &mut Vec::new(), filter.as_deref());
        }
        "float" => {
            let vpk = args.next().expect("vpk");
            let out = args.next().expect("out_dir.vpk");
            let entry = args.next().expect("entry");
            let path = parse_path(&args.next().expect("path"))?;
            let value: f32 = args.next().expect("f32-value").parse()?;
            let bytes = read_entry(&vpk, &entry)?;
            let patched = morphic::patch_kv3_resource_floats(&bytes, &[(path, value)])?;
            pack_one(&out, &entry, &patched)?;
        }
        "double" => {
            let vpk = args.next().expect("vpk");
            let out = args.next().expect("out_dir.vpk");
            let entry = args.next().expect("entry");
            let path = parse_path(&args.next().expect("path"))?;
            let value: f64 = args.next().expect("f64-value").parse()?;
            let bytes = read_entry(&vpk, &entry)?;
            let patched = morphic::patch_kv3_resource_doubles(&bytes, &[(path, value)])?;
            pack_one(&out, &entry, &patched)?;
        }
        "string" => {
            let vpk = args.next().expect("vpk");
            let out = args.next().expect("out_dir.vpk");
            let entry = args.next().expect("entry");
            let path = parse_path(&args.next().expect("path"))?;
            let value = args.next().expect("existing-string-value");
            let bytes = read_entry(&vpk, &entry)?;
            let patched = morphic::patch_kv3_resource_strings(&bytes, &[(path, value)])?;
            pack_one(&out, &entry, &patched)?;
        }
        _ => anyhow::bail!("mode must be scan, float, double, or string"),
    }
    Ok(())
}
