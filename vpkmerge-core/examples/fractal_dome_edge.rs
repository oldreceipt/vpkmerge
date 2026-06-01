// Replace the OUTLINE of Paradox's time-bomb DOME pulse with a fractal texture.
//
// The dome's rim is drawn by chrono_time_bomb_edge.vmat (pbr.vfx, additive); its
// pulse SHAPE is the alt-translucency ramp `ramp_chrono_timebomb_edge_center`
// (512x16 ATI1N), a smooth black->white->black lump that the material tiles (2x3)
// and SCROLLS around the rim -> the travelling pulse. We can't touch the material
// (it carries binary dynamic-expression blobs, like the dome surface), but the ramp
// is a separate, chrono-SPECIFIC texture, so a pure texture override is safe and
// bleeds nowhere.
//
// We swap the smooth lump for a tiling FRACTAL filament profile (a Weierstrass
// cascade -- a sum of sines at doubling frequencies, self-similar / fractal, and
// integer-frequency so it tiles exactly -> the scroll stays seamless). The rim
// pulse then reads as a jagged fractal outline instead of a smooth band.
//
// usage:
//   cargo run --release --example fractal_dome_edge -- <pak01_dir.vpk> <out_dir.vpk>
//   cargo run --release --example fractal_dome_edge -- <pak01_dir.vpk> --png <preview.png>
use morphic::{Image, ImageData};
use std::f32::consts::TAU;

const EDGE_RAMP: &str =
    "materials/particle/ramp/ramp_chrono_timebomb_edge_center_psd_41da104d.vtex_c";

// Weierstrass-like fractal: sum of sines at doubling (integer) frequencies. Tiles
// in `u` (every frequency is a whole number of cycles per unit), self-similar so
// it looks fractal at every scale. Returns 0..1.
fn weierstrass(u: f32, phase: f32) -> f32 {
    let (mut f, mut amp, mut freq, mut norm) = (0.0f32, 1.0f32, 1.0f32, 0.0f32);
    for k in 0..8 {
        f += amp * (0.5 + 0.5 * (TAU * freq * u + phase * k as f32).sin());
        norm += amp;
        amp *= 0.62;
        freq *= 2.0;
    }
    f / norm
}

// Grayscale value (0..1) for the fractal edge ramp. The pattern runs mostly along
// `u` (which maps around the rim circumference) with a mild cross-band fractal in
// `v`, so the travelling pulse breaks into fractal filaments.
fn edge_value(u: f32, v: f32) -> f32 {
    let fil = weierstrass(u, 1.7).powf(2.2); // sharpen into thin filaments
    let cross = 0.78 + 0.22 * weierstrass(v * 4.0 + u * 7.0, 0.9); // mild across-band detail
                                                                   // high gain so the brightest filaments saturate to white spikes on a dark band,
                                                                   // keeping the pulse punchy (the original peaked at full white).
    ((0.06 + 1.8 * fil) * cross).clamp(0.0, 1.0)
}

fn fill(img: &mut Image) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let ImageData::Rgba8(px) = &mut img.data else {
        anyhow::bail!("unexpected HDR ramp");
    };
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let g = (edge_value(u, v) * 255.0).round().clamp(0.0, 255.0) as u8;
            let i = ((y * w + x) * 4) as usize;
            // single-channel ATI1N (BC4) re-encode reads one channel; set all so any
            // channel choice is correct, alpha opaque.
            px[i] = g;
            px[i + 1] = g;
            px[i + 2] = g;
            px[i + 3] = 255;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a
        .next()
        .expect("usage: fractal_dome_edge <pak01_dir.vpk> <out_dir.vpk|--png file>");
    let arg2 = a.next().expect("second arg: <out_dir.vpk> or --png <file>");

    let bytes = vpkmerge_core::read_vpk_entry(&pak, EDGE_RAMP)?;
    let info = morphic::inspect(&bytes)?;
    let mut img = morphic::decode(&bytes)?;
    fill(&mut img)?;

    if arg2 == "--png" {
        let out = a.next().expect("--png needs an output path");
        let png = morphic::encode_image(&img, morphic::TextureFormat::PngRgba8888)?;
        std::fs::write(&out, &png)?;
        println!("wrote preview PNG: {out} ({}x{})", img.width, img.height);
        return Ok(());
    }
    let out = arg2;

    let new_bytes = morphic::replace_mip_chain(&bytes, &img)?;
    eprintln!(
        "fractal edge ramp re-encoded: {}x{} {:?}",
        info.width, info.height, info.format
    );

    let readme = "Paradox time-bomb DOME EDGE -> FRACTAL outline\n\
        =============================================\n\
        vpkmerge test build. Overrides the dome rim's pulse ramp\n\
        (ramp_chrono_timebomb_edge_center, 512x16 ATI1N) with a tiling fractal\n\
        filament profile (Weierstrass cascade), so the dome's scrolling pulse outline\n\
        reads as fractal instead of a smooth band. Pure texture override (the edge\n\
        material carries dynamic-expression blobs and can't be re-encoded); chrono-\n\
        specific path, no cross-effect bleed. Stacks with the prism + fractal booms.\n";
    vpkmerge_core::pack(
        &[
            (EDGE_RAMP, new_bytes.as_slice()),
            ("README.txt", readme.as_bytes()),
        ],
        &out,
    )?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
