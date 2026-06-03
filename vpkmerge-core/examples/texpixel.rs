// Print a few decoded RGBA pixels of a .vtex_c (top mip), to read neutral
// values of placeholder data textures (normal-roughness, masks).
// usage: cargo run --example texpixel -- <vpk> <entry>
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = a.next().expect("vpk");
    let entry = a.next().expect("entry");
    let bytes = vpkmerge_core::read_vpk_entry(&vpk, &entry)?;
    let info = morphic::inspect(&bytes)?;
    println!(
        "{}x{} format={:?} mips={}",
        info.width, info.height, info.format, info.mip_count
    );
    let img = morphic::decode(&bytes)?;
    let (w, h) = (img.width as usize, img.height as usize);
    let morphic::ImageData::Rgba8(px) = &img.data else {
        anyhow::bail!("HDR texture; not handled here");
    };
    let at = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        format!(
            "({:3},{:3},{:3},{:3})",
            px[i],
            px[i + 1],
            px[i + 2],
            px[i + 3]
        )
    };
    println!("px[0,0]   = {}", at(0, 0));
    println!("px[c,c]   = {}", at(w / 2, h / 2));
    println!("px[max]   = {}", at(w - 1, h - 1));
    Ok(())
}
