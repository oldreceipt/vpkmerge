// Paradox (chrono) FX PACK: stamp a procedural STRUCTURE onto every chrono ability
// texture that ships its own pattern (so it can be repainted without bleeding onto
// shared engine noise -- the dome-surface lesson). The structure is added to
// luminance + alpha (not colour), so the particle/prism tint still drives the hue;
// we're adding fractal/organic DETAIL, not recolouring.
//
// --style picks the structure: fractal (Julia traces) | liquid (marbled veins) |
// moire (interference fringes) | kaleido (mandala). Flat ground decals get a radial
// falloff so a square texture still reads as a soft circle.
//
// Targets (chrono-specific paths, real size -- from chrono_tex_scan):
//   time_bomb gear / caustic / projected   (the detonation booms, yesterday's set)
//   time_stop_ground_projected             (the ground circle UNDER the time-stop dome)
//   chrono_fx_bubble02/04_color            (the time-stop bubble shell bands)
//   status_chrono_sphere_charge_detail     (the charge sprite)
//
// usage:
//   cargo run --release --example chrono_fx_pack -- <pak01_dir.vpk> <out_dir.vpk> [--style S]
//   cargo run --release --example chrono_fx_pack -- <pak01_dir.vpk> --png <file> [--style S]
use morphic::{Image, ImageData};
use std::f32::consts::TAU;

// (entry, radial-falloff). radial=true for flat ground/sprite decals (keep them
// circular); false for model-mapped FX (bubble shell) where we want full coverage.
const TARGETS: &[(&str, bool)] = &[
    ("materials/particle/abilities/chrono/chrono_time_bomb_gear.vtex_c", true),
    ("materials/particle/abilities/chrono/chrono_time_bomb_caustic_projected_trans_psd_c629a520.vtex_c", true),
    ("materials/particle/abilities/chrono/chrono_time_bomb_projected_trans_psd_a2bf6892.vtex_c", true),
    ("materials/particle/projected/chrono_time_stop_ground_projected_psd_fec8fa92.vtex_c", true),
    ("models/heroes_staging/chrono/materials/chrono_fx_bubble02_color_psd_f57b1ef0.vtex_c", false),
    ("models/heroes_staging/chrono/materials/chrono_fx_bubble04_color_psd_ee26af5c.vtex_c", false),
    ("materials/particle/status_fx/status_chrono_sphere_charge_detail.vtex_c", true),
];

// ---- noise + fractal primitives (period-agnostic; these are stamped per texture) ----
fn hash2(i: i64, j: i64) -> f32 {
    let mut h = (i
        .wrapping_mul(374_761_393)
        .wrapping_add(j.wrapping_mul(668_265_263))) as u64;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xff_ffff) as f32 / 16_777_216.0
}
fn vnoise(x: f32, y: f32, p: i64) -> f32 {
    let (gx, gy) = (x * p as f32, y * p as f32);
    let (x0, y0) = (gx.floor() as i64, gy.floor() as i64);
    let (fx, fy) = (gx - x0 as f32, gy - y0 as f32);
    let wrap = |a: i64| ((a % p) + p) % p;
    let s = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (ux, uy) = (s(fx), s(fy));
    let v00 = hash2(wrap(x0), wrap(y0));
    let v10 = hash2(wrap(x0 + 1), wrap(y0));
    let v01 = hash2(wrap(x0), wrap(y0 + 1));
    let v11 = hash2(wrap(x0 + 1), wrap(y0 + 1));
    let a = v00 + (v10 - v00) * ux;
    let b = v01 + (v11 - v01) * ux;
    a + (b - a) * uy
}
fn fbm(x: f32, y: f32, p0: i64, oct: u32) -> f32 {
    let (mut sum, mut amp, mut p, mut norm) = (0.0, 0.5, p0, 0.0);
    for _ in 0..oct {
        sum += amp * vnoise(x, y, p);
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

// thin bright Julia filaments, 0..1
fn julia_field(u: f32, v: f32) -> f32 {
    let (mut x, mut y) = ((u - 0.5) * 3.0 - 0.8, (v - 0.5) * 3.0 + 0.156);
    let c = (-0.8, 0.156);
    let mut i = 0u32;
    while x * x + y * y <= 16.0 && i < 96 {
        let xt = x * x - y * y + c.0;
        y = 2.0 * x * y + c.1;
        x = xt;
        i += 1;
    }
    if i >= 96 {
        return 0.0;
    }
    let nu = (0.5 * (x * x + y * y).max(1e-9).ln() / TAU.ln()).ln() / TAU.ln();
    let band = ((i as f32 + 1.0 - nu) * 0.6).fract();
    (1.0 - (2.0 * band - 1.0).abs()).clamp(0.0, 1.0).powf(2.2)
}
// marbled veins, 0..1
fn liquid_field(u: f32, v: f32) -> f32 {
    let q0 = fbm(u, v, 4, 5);
    let q1 = fbm(u + 3.1, v + 6.2, 4, 5);
    let f = fbm(u + 0.5 * q0 + 1.7, v + 0.5 * q1 + 9.2, 6, 5);
    (((f * 10.0) * TAU).sin() * 0.5 + 0.5).powf(2.5)
}
// interference fringes, 0..1
fn moire_field(u: f32, v: f32) -> f32 {
    let g = |a: f32, b: f32| (TAU * (a * u + b * v)).sin();
    let m = (g(9.0, 4.0) + g(10.0, 5.0)) * (g(4.0, 9.0) + g(5.0, 8.0)) * 0.25 + 0.5;
    (1.0 - (2.0 * m - 1.0).abs()).powf(2.0)
}
// symmetric mandala, 0..1
fn kaleido_field(u: f32, v: f32) -> f32 {
    let g = |a: f32, b: f32| (TAU * (a * u + b * v)).cos();
    let m = (g(6.0, 0.0) + g(0.0, 6.0) + 0.7 * g(6.0, 6.0) + 0.7 * g(6.0, -6.0)) * 0.25;
    (m * 0.5 + 0.5).powf(2.0)
}
fn field(style: &str, u: f32, v: f32) -> f32 {
    match style {
        "liquid" => liquid_field(u, v),
        "moire" => moire_field(u, v),
        "kaleido" | "kaleidoscope" => kaleido_field(u, v),
        _ => julia_field(u, v),
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn add_structure(img: &mut Image, style: &str, radial: bool, strength: f32) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let ImageData::Rgba8(px) = &mut img.data else {
        anyhow::bail!("unexpected HDR texture");
    };
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let r = if radial {
                let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
                1.0 - smoothstep(0.40, 0.50, d)
            } else {
                1.0
            };
            let add = (field(style, u, v) * strength * r * 255.0) as i32;
            let i = ((y * w + x) * 4) as usize;
            for k in 0..4 {
                px[i + k] = (i32::from(px[i + k]) + add).clamp(0, 255) as u8;
            }
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut style = "fractal".to_string();
    let mut pos = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == "--style" {
            style = raw[i + 1].clone();
            i += 2;
        } else {
            pos.push(raw[i].clone());
            i += 1;
        }
    }
    let pak = pos
        .first()
        .cloned()
        .expect("usage: chrono_fx_pack <pak01_dir.vpk> <out_dir.vpk|--png file> [--style S]");
    let arg2 = pos
        .get(1)
        .cloned()
        .expect("second arg: <out_dir.vpk> or --png <file>");
    eprintln!("style: {style}");

    if arg2 == "--png" {
        let out = pos.get(2).cloned().expect("--png needs an output path");
        let bytes = vpkmerge_core::read_vpk_entry(&pak, TARGETS[0].0)?;
        let mut img = morphic::decode(&bytes)?;
        add_structure(&mut img, &style, TARGETS[0].1, 0.9)?;
        std::fs::write(
            &out,
            morphic::encode_image(&img, morphic::TextureFormat::PngRgba8888)?,
        )?;
        println!("wrote preview PNG: {out}");
        return Ok(());
    }
    let out = arg2;

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    for &(entry, radial) in TARGETS {
        let bytes = match vpkmerge_core::read_vpk_entry(&pak, entry) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  SKIP {entry}: {e}");
                continue;
            }
        };
        let mut img = morphic::decode(&bytes)?;
        add_structure(&mut img, &style, radial, 0.9)?;
        let new_bytes = morphic::replace_mip_chain(&bytes, &img)?;
        eprintln!(
            "  {} ({}x{}) <- {style}",
            entry.rsplit('/').next().unwrap(),
            img.width,
            img.height
        );
        packed.push((entry.to_string(), new_bytes));
    }
    anyhow::ensure!(!packed.is_empty(), "no FX textures stamped");

    let readme = format!(
        "Paradox FX PACK ({style})\n========================\n\
        vpkmerge test build. Adds a {style} STRUCTURE (luminance+alpha, additive) to\n\
        every chrono ability texture that ships its own pattern: the detonation booms\n\
        (gear/caustic/projection), the time-stop GROUND decal + bubble shell bands, and\n\
        the charge sprite. Colour stays particle/prism-driven. Shared engine noise\n\
        (the dome surface) is intentionally untouched. Pure texture override.\n"
    );
    let mut refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    refs.push(("README.txt", readme.as_bytes()));
    vpkmerge_core::pack(&refs, &out)?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
