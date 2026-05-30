// Count VPK entries matching each given prefix. usage: countprefix <vpk> <prefix>...
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(a.next().expect("vpk"))?;
    let prefixes: Vec<String> = a.collect();
    let paths: Vec<&String> = vpk.file_paths().collect();
    for pre in &prefixes {
        let n = paths.iter().filter(|p| p.starts_with(pre.as_str())).count();
        let vpcf = paths
            .iter()
            .filter(|p| p.starts_with(pre.as_str()) && p.ends_with(".vpcf_c"))
            .count();
        println!("{pre}  total={n}  vpcf_c={vpcf}");
    }
    Ok(())
}
