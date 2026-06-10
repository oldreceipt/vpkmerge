// Dump a .vmat_c's referenced textures + shader params (ASCII strings) from a VPK.
// usage: cargo run --release --example dump_vmat -- <pak.vpk> <entry.vmat_c>
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().expect("pak");
    let entry = a.next().expect("entry");
    let bytes = vpkmerge_core::read_vpk_entry(&pak, &entry)?;
    eprintln!("{} bytes", bytes.len());
    // crude ASCII string scan
    let mut cur = Vec::new();
    let mut out = Vec::new();
    for &b in &bytes {
        if (0x20..0x7f).contains(&b) {
            cur.push(b);
        } else {
            if cur.len() >= 4 {
                out.push(String::from_utf8_lossy(&cur).into_owned());
            }
            cur.clear();
        }
    }
    for s in out {
        let l = s.to_lowercase();
        if l.contains("g_t")
            || l.contains("g_v")
            || l.contains("g_f")
            || l.contains("selfillum")
            || l.contains("f_")
            || l.contains(".vtex")
            || l.contains("shader")
            || l.contains("vfx")
        {
            println!("{s}");
        }
    }
    Ok(())
}
