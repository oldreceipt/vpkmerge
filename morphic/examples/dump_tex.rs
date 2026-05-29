//  Throwaway: decode every `.vtex_c` whose path contains a substring to PNG.
//  Usage: cargo run -p morphic --example dump_tex -- <vpk> <substr> <outdir>
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (vpk_path, needle, outdir) = (&args[1], args[2].to_lowercase(), &args[3]);
    std::fs::create_dir_all(outdir).unwrap();
    let vpk = valve_pak::open(Path::new(vpk_path)).expect("open vpk");

    let mut matches: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vtex_c") && p.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    matches.sort();
    println!("{} matching .vtex_c entries:", matches.len());

    for path in &matches {
        let mut vf = match vpk.get_file(path) {
            Ok(f) => f,
            Err(e) => {
                println!("  SKIP {path}: open {e:?}");
                continue;
            }
        };
        let bytes = vf.read_all().expect("read");
        match morphic::decode(&bytes) {
            Ok(img) => match img.data {
                morphic::ImageData::Rgba8(d) => {
                    let base = path.rsplit('/').next().unwrap().replace(".vtex_c", "");
                    let out = format!("{outdir}/{base}.png");
                    image::RgbaImage::from_raw(img.width, img.height, d)
                        .unwrap()
                        .save(&out)
                        .unwrap();
                    println!("  OK   {} ({}x{}) -> {out}", path, img.width, img.height);
                }
                morphic::ImageData::Rgba16F(_) => println!("  HDR  {path} (skipped, f16)"),
            },
            Err(e) => println!("  FAIL {path}: {e:?}"),
        }
    }
}
