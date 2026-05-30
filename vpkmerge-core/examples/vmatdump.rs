// Dump a .vmat_c's shader + all params (trying every value key).
// usage: cargo run --example vmatdump -- <vpk> <entry>
use morphic::kv3::Value;

fn val(it: &Value) -> String {
    for k in ["m_value", "m_nValue", "m_flValue", "m_pValue"] {
        if let Some(v) = it.get(k) {
            return format!("{k}={v:?}");
        }
    }
    "?".into()
}

fn dump(label: &str, v: &Value) {
    if let Some(arr) = v.get(label).and_then(Value::as_array) {
        for it in arr {
            let name = match it.get("m_name") {
                Some(Value::String(s)) => s.clone(),
                _ => "?".into(),
            };
            println!("  {label}: {name} = {}", val(it));
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(&a.next().expect("vpk"))?;
    let bytes = vpk
        .get_file(&a.next().expect("entry"))
        .expect("entry")
        .read_all()?;
    let v = morphic::decode_kv3_resource(&bytes)?;
    if let Some(s) = v.get("m_shaderName") {
        println!("shader: {s:?}");
    }
    for l in [
        "m_intParams",
        "m_floatParams",
        "m_vectorParams",
        "m_textureParams",
        "m_dynamicParams",
    ] {
        dump(l, &v);
    }
    Ok(())
}
