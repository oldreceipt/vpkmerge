// Print the wrap-mode (SUGGEST_CLAMP) flags of particle textures, to decide which
// texture inputs can have their UV offset animated without smearing a clamp edge.
//
// usage:
//   cargo run -p vpkmerge-core --example texflags -- <vpk> <m_hTexture>...
// m_hTexture values are the ".vtex" paths from a particle; this appends "_c".
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let vpk = valve_pak::open(&args.next().expect("vpk"))?;
    for h in args {
        let entry = if h.ends_with(".vtex_c") {
            h.clone()
        } else if h.ends_with(".vtex") {
            format!("{h}_c")
        } else {
            format!("{h}.vtex_c")
        };
        let Ok(mut f) = vpk.get_file(&entry) else {
            println!("MISSING  {entry}");
            continue;
        };
        let bytes = f.read_all()?;
        match morphic::inspect(&bytes) {
            Ok(info) => {
                // SUGGEST_CLAMP_S = bit 0, SUGGEST_CLAMP_T = bit 1 (VTexFlags).
                let raw = info.flags.bits();
                let clamp_s = raw & (1 << 0) != 0;
                let clamp_t = raw & (1 << 1) != 0;
                let wrap = if clamp_s || clamp_t {
                    "CLAMP (offset unsafe)"
                } else {
                    "REPEAT (offset ok)"
                };
                println!(
                    "{wrap:22}  clampS={clamp_s} clampT={clamp_t}  {}x{}  {}",
                    info.width,
                    info.height,
                    entry.rsplit('/').next().unwrap_or(&entry)
                );
            }
            Err(e) => println!("ERR {e:?}  {entry}"),
        }
    }
    Ok(())
}
