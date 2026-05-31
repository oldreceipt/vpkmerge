// Add psychedelic FRACTAL TRACES to Paradox's time-bomb boom textures and pack
// them into an addon VPK. Pure texture override at the bomb's existing texture
// paths, so the bomb particle systems render the fractal automatically; the prism
// addon (pak06) already tints these sprites rainbow, so together the boom reads as
// flowing rainbow fractal traces. We can't author new fractal GEOMETRY/emitters
// (that needs a particle re-encode, which breaks in-game), but the boom sprites/
// decals take on fractal structure via their textures.
//
// Targets (all BC7, chrono-specific so no cross-hero bleed):
//   chrono_time_bomb_gear           (512)  the clock-gear sprite
//   chrono_time_bomb_caustic_..     (1024) ground caustic decal (pattern in alpha)
//   chrono_time_bomb_projected_..   (1024) ground projection (pattern in alpha)
//
// usage:
//   cargo run --release --example fractal_bomb -- <pak01_dir.vpk> <out_dir.vpk>
//   cargo run --release --example fractal_bomb -- <pak01_dir.vpk> --png <preview.png>
use morphic::{Image, ImageData};

const GEAR: &str = "materials/particle/abilities/chrono/chrono_time_bomb_gear.vtex_c";
const CAUSTIC: &str =
    "materials/particle/abilities/chrono/chrono_time_bomb_caustic_projected_trans_psd_c629a520.vtex_c";
const PROJECTED: &str =
    "materials/particle/abilities/chrono/chrono_time_bomb_projected_trans_psd_a2bf6892.vtex_c";

// Julia-set "trace" field: thin bright filaments following the fractal structure,
// dark elsewhere. `c` picks the Julia shape; `zoom`/`ox`/`oy` frame it.
fn julia_trace(u: f32, v: f32, c: (f32, f32), zoom: f32, ox: f32, oy: f32) -> f32 {
    let mut x = (u - 0.5) * zoom + ox;
    let mut y = (v - 0.5) * zoom + oy;
    let max = 96u32;
    let mut i = 0u32;
    while x * x + y * y <= 16.0 && i < max {
        let xt = x * x - y * y + c.0;
        y = 2.0 * x * y + c.1;
        x = xt;
        i += 1;
    }
    if i >= max {
        return 0.0; // inside the set -> no trace (dark)
    }
    // smooth (fractional) iteration count
    let mag2 = (x * x + y * y).max(1e-9);
    let log2 = std::f32::consts::LN_2;
    let nu = (0.5 * mag2.ln() / log2).ln() / log2;
    let smooth = i as f32 + 1.0 - nu;
    // iteration bands -> a triangle pulse makes them THIN bright contour traces
    let band = (smooth * 0.6).fract();
    let tri = 1.0 - (2.0 * band - 1.0).abs();
    tri.clamp(0.0, 1.0).powf(2.2)
}

fn rgba8_mut(img: &mut Image) -> anyhow::Result<&mut Vec<u8>> {
    match &mut img.data {
        ImageData::Rgba8(v) => Ok(v),
        ImageData::Rgba16F(_) => anyhow::bail!("unexpected HDR texture"),
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Add fractal traces ADDITIVELY into both luminance (RGB) and coverage (alpha), so
// the boom gains glowing fractal filaments (fuller, not eroded). The original
// sprite/decal shape is preserved; traces layer on top.
//
// A RADIAL falloff fades the traces to zero before the texture edge, so a square
// ground-projection texture still reads as a soft circle (otherwise the additive
// fractal fills the corners and the square decal boundary shows on the ground).
fn add_fractal(
    img: &mut Image,
    c: (f32, f32),
    zoom: f32,
    ox: f32,
    oy: f32,
    strength: f32,
) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            // distance from center, normalized so the inscribed circle edge ~= 0.5
            let dist = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
            let radial = 1.0 - smoothstep(0.40, 0.50, dist); // 1 in center, 0 by the edge
            let t = julia_trace(u, v, c, zoom, ox, oy) * strength * radial;
            let add = (t * 255.0) as i32;
            let i = ((y * w + x) * 4) as usize;
            for k in 0..4 {
                let nv = i32::from(px[i + k]) + add;
                px[i + k] = nv.clamp(0, 255) as u8;
            }
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a
        .next()
        .expect("usage: fractal_bomb <pak01_dir.vpk> <out_dir.vpk|--png file>");
    let arg2 = a.next().expect("second arg: <out_dir.vpk> or --png <file>");

    // (entry, julia c, zoom, offset x, offset y, strength)
    let jobs: &[(&str, (f32, f32), f32, f32, f32, f32)] = &[
        (GEAR, (-0.8, 0.156), 3.0, 0.0, 0.0, 0.9), // dendrite filaments
        (CAUSTIC, (0.285, 0.01), 2.6, 0.0, 0.0, 0.85), // near-Siegel-disk swirls
        (PROJECTED, (-0.70176, -0.3842), 3.0, 0.0, 0.0, 0.85), // spiral filaments
    ];

    if arg2 == "--png" {
        let out = a.next().expect("--png needs an output path");
        let (entry, c, zoom, ox, oy, k) = jobs[0];
        let bytes = vpkmerge_core::read_vpk_entry(&pak, entry)?;
        let mut img = morphic::decode(&bytes)?;
        add_fractal(&mut img, c, zoom, ox, oy, k)?;
        let png = morphic::encode_image(&img, morphic::TextureFormat::PngRgba8888)?;
        std::fs::write(&out, &png)?;
        println!("wrote preview PNG: {out} ({}x{})", img.width, img.height);
        return Ok(());
    }
    let out = arg2;

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    for &(entry, c, zoom, ox, oy, k) in jobs {
        let bytes = vpkmerge_core::read_vpk_entry(&pak, entry)?;
        let mut img = morphic::decode(&bytes)?;
        add_fractal(&mut img, c, zoom, ox, oy, k)?;
        let new_bytes = morphic::replace_mip_chain(&bytes, &img)?;
        eprintln!(
            "fractal traces added: {} ({}x{})",
            entry.rsplit('/').next().unwrap(),
            img.width,
            img.height
        );
        packed.push((entry.to_string(), new_bytes));
    }

    // NOTE: the bomb DOME (models/particle/sphere.vmdl + chrono_time_bomb_sphere.vmat)
    // can't be made fractal cleanly: its material carries binary-blob dynamic-expression
    // params (animated selfillum), so the byte-faithful string-repoint can't rewrap it,
    // and a full re-encode renders as the engine error shader. Its surface texture is the
    // SHARED noise_caustic, so overriding that would bleed onto every caustic effect.
    // Left for a future approach (e.g. a fractal volume particle, or particle-density boost).

    let readme = "Paradox time-bomb FRACTAL TRACES\n\
        ================================\n\
        vpkmerge test build. Adds Julia-set fractal filaments to the time-bomb boom\n\
        textures (gear + ground caustic/projection). Pure texture override; rides the\n\
        prism (pak06) particle color, so the booms read as flowing rainbow fractal\n\
        traces. No new geometry/emitters (those need a particle re-encode that breaks).\n";

    let mut refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    refs.push(("README.txt", readme.as_bytes()));
    vpkmerge_core::pack(&refs, &out)?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
