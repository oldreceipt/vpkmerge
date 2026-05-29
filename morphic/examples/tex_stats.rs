//  Throwaway: per-channel stats of a decoded .vtex_c (find masks in alpha).
//  Usage: cargo run -p morphic --example tex_stats -- <vpk> <substr>
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vpk = valve_pak::open(Path::new(&a[1])).unwrap();
    let needle = a[2].to_lowercase();
    let mut paths: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vtex_c") && p.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    paths.sort();
    for path in &paths {
        let bytes = vpk.get_file(path).unwrap().read_all().unwrap();
        match morphic::decode(&bytes) {
            Ok(img) => {
                if let morphic::ImageData::Rgba8(d) = img.data {
                    let n = f64::from(u32::try_from(d.len() / 4).unwrap());
                    let mut mn = [255u8; 4];
                    let mut mx = [0u8; 4];
                    let mut sum = [0f64; 4];
                    let mut uniq = [
                        std::collections::HashSet::new(),
                        std::collections::HashSet::new(),
                        std::collections::HashSet::new(),
                        std::collections::HashSet::new(),
                    ];
                    for px in d.chunks_exact(4) {
                        for c in 0..4 {
                            mn[c] = mn[c].min(px[c]);
                            mx[c] = mx[c].max(px[c]);
                            sum[c] += f64::from(px[c]);
                            if uniq[c].len() < 300 {
                                uniq[c].insert(px[c]);
                            }
                        }
                    }
                    let short = path.rsplit('/').next().unwrap();
                    println!("\n{}  {}x{}", short, img.width, img.height);
                    for c in 0..4 {
                        println!(
                            "  {}: min={} max={} mean={:.1} uniq~{}",
                            "RGBA".as_bytes()[c] as char,
                            mn[c],
                            mx[c],
                            sum[c] / n,
                            uniq[c].len()
                        );
                    }
                }
            }
            Err(e) => println!("{path}: FAIL {e:?}"),
        }
    }
}
