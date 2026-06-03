// Validate that every resource in a baked prism VPK still decodes and (for the
// Sweep animation pass, which only patches leaf values in place) keeps the same
// KV3 structure as the base. A decode failure or a structural divergence means a
// byte-faithful patch landed wrong and corrupted the file: precisely the kind of
// malformed resource that hard-crashes the engine loader on precache.
//
// usage: cargo run --example validate_prism_bake -- <baked_dir.vpk> <base_dir.vpk>
use morphic::kv3::Value;

fn raw(vpk: &valve_pak::VPK, entry: &str) -> Option<Vec<u8>> {
    let mut f = vpk.get_file(entry).ok()?;
    f.read_all().ok()
}

/// Structural equality ignoring leaf values: same object keys in order, same
/// array lengths, recursively. Sweep edits only change leaf scalars/strings, so a
/// correct bake is shape-identical to base; a wrong-offset patch is not.
fn same_shape(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(pa), Value::Object(pb)) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb)
                    .all(|((ka, va), (kb, vb))| ka == kb && same_shape(va, vb))
        }
        (Value::Array(ia), Value::Array(ib)) => {
            ia.len() == ib.len() && ia.iter().zip(ib).all(|(x, y)| same_shape(x, y))
        }
        (Value::Object(_), _) | (_, Value::Object(_)) => false,
        (Value::Array(_), _) | (_, Value::Array(_)) => false,
        _ => true,
    }
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let modv = valve_pak::open(a.next().expect("baked vpk"))?;
    let base = valve_pak::open(a.next().expect("base vpk"))?;

    let mut paths: Vec<String> = modv.file_paths().cloned().collect();
    paths.sort();

    let (mut vpcf_ok, mut vpcf_shape_diff, mut vpcf_fail, mut vpcf_new) = (0u32, 0u32, 0u32, 0u32);
    let (mut vtex_ok, mut vtex_fail, mut vtex_dim_diff) = (0u32, 0u32, 0u32);
    let mut other = 0u32;
    let mut problems: Vec<String> = Vec::new();

    for p in &paths {
        let Some(bytes) = raw(&modv, p) else {
            problems.push(format!("READ-FAIL          {p}"));
            continue;
        };

        if p.ends_with(".vpcf_c") {
            match morphic::decode_kv3_resource(&bytes) {
                Err(e) => {
                    vpcf_fail += 1;
                    problems.push(format!("VPCF-DECODE-FAIL   {p}: {e}"));
                }
                Ok(modtree) => {
                    if !matches!(modtree, Value::Object(_)) {
                        vpcf_fail += 1;
                        problems.push(format!("VPCF-NOT-OBJECT    {p}"));
                        continue;
                    }
                    match raw(&base, p).and_then(|b| morphic::decode_kv3_resource(&b).ok()) {
                        None => vpcf_new += 1,
                        Some(basetree) => {
                            if same_shape(&basetree, &modtree) {
                                vpcf_ok += 1;
                            } else {
                                vpcf_shape_diff += 1;
                                problems.push(format!("VPCF-SHAPE-DIFF    {p}"));
                            }
                        }
                    }
                }
            }
        } else if p.ends_with(".vtex_c") {
            match morphic::inspect(&bytes) {
                Err(e) => {
                    vtex_fail += 1;
                    problems.push(format!("VTEX-INSPECT-FAIL  {p}: {e}"));
                }
                Ok(info) => match morphic::decode(&bytes) {
                    Err(e) => {
                        vtex_fail += 1;
                        problems.push(format!("VTEX-DECODE-FAIL   {p}: {e}"));
                    }
                    Ok(img) => {
                        if img.width == 0 || img.height == 0 {
                            vtex_fail += 1;
                            problems.push(format!("VTEX-ZERO-DIM      {p}"));
                        } else if let Some(bi) =
                            raw(&base, p).and_then(|b| morphic::inspect(&b).ok())
                        {
                            let dim_ok = bi.width == info.width && bi.height == info.height;
                            let fmt_ok = bi.format == info.format;
                            let mip_ok = bi.mip_count == info.mip_count;
                            if dim_ok && fmt_ok && mip_ok {
                                vtex_ok += 1;
                            } else {
                                vtex_dim_diff += 1;
                                problems.push(format!(
                                    "VTEX-META-DIFF     {p}: base {}x{} {:?} mips={} -> bake {}x{} {:?} mips={}",
                                    bi.width, bi.height, bi.format, bi.mip_count,
                                    info.width, info.height, info.format, info.mip_count
                                ));
                            }
                        } else {
                            vtex_ok += 1;
                        }
                    }
                },
            }
        } else {
            other += 1;
        }
    }

    println!("entries: {}", paths.len());
    println!(
        "  .vpcf_c: ok={vpcf_ok} new={vpcf_new} SHAPE-DIFF={vpcf_shape_diff} DECODE-FAIL={vpcf_fail}"
    );
    println!("  .vtex_c: ok={vtex_ok} DIM-DIFF={vtex_dim_diff} FAIL={vtex_fail}");
    println!("  other:   {other}");
    if problems.is_empty() {
        println!("\nNO PROBLEMS: every resource decoded and kept its base structure.");
    } else {
        println!("\n{} PROBLEM(S):", problems.len());
        for line in &problems {
            println!("  {line}");
        }
    }
    Ok(())
}
