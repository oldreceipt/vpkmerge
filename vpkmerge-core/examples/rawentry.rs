// Dump one raw VPK entry to a file. usage: rawentry <vpk> <entry> <out>
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(a.next().expect("vpk"))?;
    let entry = a.next().expect("entry");
    let out = a.next().expect("out");
    let bytes = vpk.get_file(&entry).expect("entry").read_all()?;
    std::fs::write(&out, &bytes)?;
    eprintln!("wrote {} ({} bytes)", out, bytes.len());
    Ok(())
}
