// Decode a .vtex_c (top mip) from a VPK entry to a raw PNG, no recolor.
// usage: cargo run --example decodetex -- <vpk> <entry> <out.png>
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = a.next().expect("vpk");
    let entry = a.next().expect("entry");
    let out = a.next().expect("out png");
    let bytes = vpkmerge_core::read_vpk_entry(&vpk, &entry)?;
    let info = morphic::inspect(&bytes)?;
    eprintln!(
        "{entry}\n  {}x{} format={:?} mips={}",
        info.width, info.height, info.format, info.mip_count
    );
    let img = morphic::decode(&bytes)?;
    let png = morphic::encode_image(&img, morphic::TextureFormat::PngRgba8888)?;
    std::fs::write(&out, png)?;
    eprintln!("  wrote {out}");
    Ok(())
}
