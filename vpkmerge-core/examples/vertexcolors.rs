// Diagnostic for the vertex-color recolor (Paige ult horse/knight): list every
// model matching a substring, and for each vertex buffer report whether it
// carries a COLOR attribute, its on-disk format, and hue/saturation stats over
// the baked per-vertex colors. Confirms (or refutes) the handoff's assumption
// that the horse's green is baked into mesh vertex colors.
//
// usage: cargo run --example vertexcolors -- <vpk> <name-substring>   (e.g. bookworm)
use morphic::model::{read_vertex_colors, vertex_targets};

/// RGB (0..1) -> (hue degrees, saturation, value). Matches core recolor's HSV.
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk_path = a.next().expect("vpk");
    let needle = a.next().expect("name substring, e.g. bookworm");
    let vpk = valve_pak::open(&vpk_path)?;

    let models: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vmdl_c") && p.contains(&needle))
        .cloned()
        .collect();
    println!("{} model(s) matching {needle:?}\n", models.len());

    for entry in &models {
        let mut f = vpk.get_file(entry).expect("entry");
        let bytes = f.read_all()?;
        let targets = match vertex_targets(&bytes) {
            Ok(t) => t,
            Err(e) => {
                println!("{entry}\n  (vertex_targets failed: {e})\n");
                continue;
            }
        };
        let color_bufs: Vec<_> = targets.iter().filter(|t| t.has_color).collect();
        if color_bufs.is_empty() {
            continue; // only report models that carry vertex colors
        }
        println!("{entry}");
        for t in color_bufs {
            let colors = match read_vertex_colors(&bytes, t.block_index)? {
                Some(c) => c,
                None => continue,
            };
            // Saturation-weighted mean hue (low-sat vertices have noisy hue), plus
            // mean sat/value and the fraction that is "greenish" (hue 80..180, sat>0.2).
            let (mut hx, mut hy, mut wsum) = (0f32, 0f32, 0f32);
            let (mut ssum, mut vsum) = (0f32, 0f32);
            let mut green = 0usize;
            for c in &colors {
                let (h, s, v) = rgb_to_hsv(c[0], c[1], c[2]);
                let rad = h.to_radians();
                hx += s * rad.cos();
                hy += s * rad.sin();
                wsum += s;
                ssum += s;
                vsum += v;
                if s > 0.2 && (80.0..=180.0).contains(&h) {
                    green += 1;
                }
            }
            let n = colors.len().max(1) as f32;
            let mean_hue = if wsum > 1e-6 {
                hy.atan2(hx).to_degrees().rem_euclid(360.0)
            } else {
                f32::NAN
            };
            // First few raw samples (as 0..255) for a sanity eyeball.
            let sample: Vec<[u8; 4]> = colors
                .iter()
                .take(3)
                .map(|c| {
                    [
                        (c[0] * 255.0).round() as u8,
                        (c[1] * 255.0).round() as u8,
                        (c[2] * 255.0).round() as u8,
                        (c[3] * 255.0).round() as u8,
                    ]
                })
                .collect();
            println!(
                "  mesh {:<16} block {:>3}  {} verts  meshopt={} editable={} stride={}  COLOR  mean_hue={:6.1}  mean_sat={:.2}  mean_val={:.2}  green={:.0}%  e.g. {:?}",
                t.mesh_name,
                t.block_index,
                t.vertex_count,
                t.meshopt,
                t.editable,
                t.stride,
                mean_hue,
                ssum / n,
                vsum / n,
                100.0 * green as f32 / n,
                sample,
            );
        }
        println!();
    }
    Ok(())
}
