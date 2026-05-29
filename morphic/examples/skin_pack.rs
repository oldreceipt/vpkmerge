//  Throwaway: splice an edited PNG into an existing .vtex_c and pack an addon VPK.
//  Usage: cargo run -p morphic --example skin_pack -- <src_vpk> <vtex_entry> <png> <workdir> <out_vpk>
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (src_vpk, entry, png, workdir, out_vpk) = (&a[1], &a[2], &a[3], &a[4], &a[5]);

    // 1. original compiled texture from the game pak
    let vpk = valve_pak::open(Path::new(src_vpk)).expect("open src vpk");
    let orig = vpk
        .get_file(entry)
        .expect("entry")
        .read_all()
        .expect("read");
    let info = morphic::inspect(&orig).expect("inspect");
    println!(
        "original: {:?} {}x{} mips={}",
        info.format, info.width, info.height, info.mip_count
    );

    // 2. decode original to recover its alpha channel (Source albedo alpha is a mask)
    let dec = morphic::decode(&orig).expect("decode orig");
    let orig_alpha: Vec<u8> = match dec.data {
        morphic::ImageData::Rgba8(d) => d.iter().skip(3).step_by(4).copied().collect(),
        morphic::ImageData::Rgba16F(_) => panic!("orig not rgba8"),
    };

    // 3. load the edited PNG (RGBA8)
    let edited = image::open(Path::new(png)).expect("open png").to_rgba8();
    let (w, h) = edited.dimensions();
    assert_eq!(
        (w, h),
        (u32::from(info.width), u32::from(info.height)),
        "png size must match texture"
    );
    let mut buf = edited.into_raw();
    // preserve original alpha so the shader's mask channel is unchanged
    for (i, px) in buf.chunks_exact_mut(4).enumerate() {
        px[3] = orig_alpha[i];
    }
    let img = morphic::Image {
        width: w,
        height: h,
        data: morphic::ImageData::Rgba8(buf),
    };

    // 4. splice: re-encode in the texture's native format + rebuild the mip chain
    let new_vtex = morphic::replace_mip_chain(&orig, &img).expect("splice");
    println!(
        "spliced .vtex_c: {} bytes (orig {} bytes)",
        new_vtex.len(),
        orig.len()
    );

    // 5. pack an addon VPK with the new texture at the SAME internal path (override)
    let root = Path::new(workdir);
    let dst = root.join(entry);
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::write(&dst, &new_vtex).unwrap();
    let packed = valve_pak::from_directory(root).expect("pack");
    packed.save(Path::new(out_vpk)).expect("save");
    println!("wrote addon VPK: {out_vpk}");
}
