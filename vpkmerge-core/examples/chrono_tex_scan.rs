// Scan Paradox (chrono) textures and classify each as a fractal-able PATTERN
// target vs an un-overridable shared/placeholder. The rule (learned from the dome):
// we can only repaint a texture whose PATH is chrono-specific (overriding it can't
// bleed onto other heroes) AND that carries real content (not a 4x4 placeholder).
//
// A texture is chrono-specific if its path names the hero (chrono / chrno). Shared
// engine textures (materials/particle/noise/, /default/, warp_spiral, voronoi, ...)
// are reported separately as OFF-LIMITS. Size is read from the resource header only
// (morphic::inspect), so this is fast.
//
// usage: cargo run --release --example chrono_tex_scan -- <pak01_dir.vpk>
fn is_chrono(path: &str) -> bool {
    path.contains("chrono") || path.contains("chrno")
}

// Name keywords that mark a texture as a lighting MAP (normal/ao/metal/mask), not a
// visible color/pattern. We can still override these, but they shape relief/glow,
// not the painted look.
fn is_map(path: &str) -> bool {
    let p = path.to_lowercase();
    [
        "normalroughness",
        "ambientocclusion",
        "metalness",
        "_ao_",
        "_normal",
        "tintmask",
    ]
    .iter()
    .any(|k| p.contains(k))
}

fn is_mask(path: &str) -> bool {
    path.to_lowercase().contains("selfillummask") || path.to_lowercase().contains("mask")
}

fn main() -> anyhow::Result<()> {
    let pak = std::env::args()
        .nth(1)
        .expect("usage: chrono_tex_scan <pak01_dir.vpk>");
    let paths: Vec<String> = vpkmerge_core::inspect(&pak)?
        .file_paths
        .into_iter()
        .filter(|p| p.ends_with(".vtex_c") && is_chrono(p))
        .collect();

    let mut pattern: Vec<(u32, u32, String, String)> = Vec::new(); // (w,h,fmt,path) real color/pattern
    let mut maps: Vec<(u32, u32, String)> = Vec::new();
    let mut placeholders = 0usize;

    for p in &paths {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, p) else {
            continue;
        };
        let Ok(info) = morphic::inspect(&bytes) else {
            continue;
        };
        if info.width <= 8 {
            placeholders += 1;
            continue;
        }
        let fmt = format!("{:?}", info.format);
        let (w, h) = (u32::from(info.width), u32::from(info.height));
        if is_map(p) || (is_mask(p) && w <= 64) {
            maps.push((w, h, p.clone()));
        } else {
            pattern.push((w, h, fmt, p.clone()));
        }
    }

    pattern.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
    println!(
        "== FRACTAL-ABLE PATTERN textures ({} found) ==",
        pattern.len()
    );
    println!("   (chrono-specific path + real size -> safe to repaint, carries the look)");
    for (w, h, fmt, p) in &pattern {
        println!(
            "  {:>4}x{:<4} {:<8} {}",
            w,
            h,
            fmt,
            p.trim_start_matches("materials/particle/")
        );
    }

    maps.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
    println!("\n== lighting MAPS / masks (overridable, shape relief/glow not color) ==");
    for (w, h, p) in &maps {
        println!(
            "  {:>4}x{:<4} {}",
            w,
            h,
            p.trim_start_matches("materials/particle/")
        );
    }
    println!("\n{placeholders} chrono textures are 4x4-ish placeholders (skipped).");
    Ok(())
}
