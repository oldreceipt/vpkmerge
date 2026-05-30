// Find .vmdl references inside .vpcf_c particle files under a prefix.
// usage: cargo run --example modelrefs -- <vpk> <prefix>
use morphic::kv3::Value;
use std::collections::BTreeMap;
fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.iter().for_each(|(_, x)| strings(x, out)),
        _ => {}
    }
}
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(&a.next().unwrap())?;
    let prefix = a.next().unwrap();
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in vpk
        .file_paths()
        .filter(|p| p.starts_with(&prefix) && p.ends_with(".vpcf_c"))
        .cloned()
        .collect::<Vec<_>>()
    {
        let b = vpk.get_file(&p).unwrap().read_all()?;
        let Ok(v) = morphic::decode_kv3_resource(&b) else {
            continue;
        };
        let mut ss = Vec::new();
        strings(&v, &mut ss);
        for s in ss.iter().filter(|s| s.contains(".vmdl")) {
            refs.entry(s.clone())
                .or_default()
                .push(p.rsplit('/').next().unwrap().into());
        }
    }
    for (m, users) in &refs {
        println!(
            "{m}\n    <- {} particle(s) e.g. {:?}",
            users.len(),
            &users[..users.len().min(3)]
        );
    }
    Ok(())
}
