// Dev tool: compare morphic's inline-PNG decode against the oracle golden
// pixel-by-pixel. Run: cargo run --example dump_inline_diff -p morphic.

use std::fs;

fn main() {
    let vt = fs::read("morphic/fixtures/png_rgba8888/dynamic_images_sentinel.vtex_c").unwrap();
    let golden = image::load_from_memory(
        &fs::read("morphic/fixtures/png_rgba8888/dynamic_images_sentinel.png").unwrap(),
    )
    .unwrap()
    .to_rgba8();
    let info = morphic::inspect(&vt).unwrap();
    let img = morphic::decode(&vt).unwrap();
    println!(
        "morphic: {}x{}, info {:?} dims={}x{}",
        img.width, img.height, info.format, info.width, info.height
    );
    println!("golden:  {}x{}", golden.width(), golden.height());
    let morphic::ImageData::Rgba8(buf) = &img.data else {
        return;
    };

    let mut diffs = 0u32;
    let mut diffs_at_alpha0 = 0u32;
    let mut diffs_at_partial = 0u32;
    let mut max_diff = 0u8;
    let mut max_at = (0u32, 0u32);
    let mut partial_examples: Vec<(u32, u32)> = Vec::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let i = (y as usize * img.width as usize + x as usize) * 4;
            let g = golden.get_pixel(x, y);
            let a_morphic = buf[i + 3];
            let a_golden = g[3];
            for c in 0..4 {
                let d = buf[i + c].abs_diff(g[c]);
                if d > 0 {
                    diffs += 1;
                    if a_golden == 0 && a_morphic == 0 {
                        diffs_at_alpha0 += 1;
                    } else {
                        diffs_at_partial += 1;
                        if partial_examples.len() < 5 {
                            partial_examples.push((x, y));
                        }
                    }
                }
                if d > max_diff {
                    max_diff = d;
                    max_at = (x, y);
                }
            }
        }
    }
    println!(
        "channel diffs: total {diffs}, at A=0 {diffs_at_alpha0}, at A>0 {diffs_at_partial}, max {max_diff} at {max_at:?}"
    );
    for (x, y) in partial_examples {
        let i = (y as usize * img.width as usize + x as usize) * 4;
        let g = golden.get_pixel(x, y);
        println!(
            "  partial-alpha diff ({x},{y}): morphic=[{} {} {} {}] golden=[{} {} {} {}]",
            buf[i],
            buf[i + 1],
            buf[i + 2],
            buf[i + 3],
            g[0],
            g[1],
            g[2],
            g[3]
        );
    }
    for &(x, y) in &[(0u32, 0u32), (10, 0), (46, 26), (60, 26), (91, 51)] {
        let i = (y as usize * img.width as usize + x as usize) * 4;
        let g = golden.get_pixel(x, y);
        println!(
            "  ({x},{y}) morphic=[{} {} {} {}] golden=[{} {} {} {}]",
            buf[i],
            buf[i + 1],
            buf[i + 2],
            buf[i + 3],
            g[0],
            g[1],
            g[2],
            g[3]
        );
    }
}
