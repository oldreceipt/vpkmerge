// Build the Vindicta "op-art optical illusion" dress reskin and pack it into an addon VPK.
//
// Overrides two textures of `vindicta_dress.vmat` (no material edit, pure texture override):
//   g_tColor            -> bold tiling op-art (Riley wavy stripes), cream vs deep-maroon
//   g_tNormalRoughness  -> flat normal (RG=128) + op-art roughness in B (cream bands glossy)
// Both are tiling/seamless so they survive the dress mesh's scrambled, overlapping UVs.
//
// usage: cargo run --release --example reskin_dress -- <pak01_dir.vpk> <out_dir.vpk>
use morphic::{Image, ImageData};

const COLOR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_color_png_a192a2cd.vtex_c";
const NORMALROUGH: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_vmat_g_tnormalroughness_ce38f34.vtex_c";
// 2048^2 BC7 linear-normal texture reused only as the encode container for the roughness map.
const NR_DONOR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_normal_png_f4a1a6e6.vtex_c";

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Op-art field: wavy vertical stripes. v in [0,1]: 1 = cream stripe, 0 = dark.
// Seamless in both axes: integer stripe + wave counts make it tile across UV islands.
fn opart(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let stripes = 22.0_f32; // vertical band count across U (integer -> seamless in U)
    let wave_m = 6.0_f32; // vertical wave count (integer -> seamless in V)
    let amp = 0.42_f32; // wave displacement, in stripe-periods
    let tau = std::f32::consts::TAU;
    let u = x as f32 / w as f32;
    let vv = y as f32 / h as f32;
    let disp = amp * (tau * wave_m * vv).sin();
    let s = (tau * (u * stripes + disp)).sin();
    smoothstep(-0.12, 0.12, s)
}

fn rgba8_mut(img: &mut Image) -> anyhow::Result<&mut Vec<u8>> {
    match &mut img.data {
        ImageData::Rgba8(v) => Ok(v),
        ImageData::Rgba16F(_) => anyhow::bail!("unexpected HDR texture"),
    }
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().expect("pak01_dir.vpk");
    let out = a.next().expect("out_dir.vpk");

    let color_bytes = vpkmerge_core::read_vpk_entry(&pak, COLOR)?;
    let nr_donor_bytes = vpkmerge_core::read_vpk_entry(&pak, NR_DONOR)?;

    // --- albedo: paint the op-art straight in (bold) ---
    let mut color = morphic::decode(&color_bytes)?;
    let (w, h) = (color.width, color.height);
    let cream = [233.0_f32, 224.0, 206.0];
    let dark = [30.0_f32, 13.0, 18.0]; // near-black, faint maroon nod to Vindicta
    {
        let px = rgba8_mut(&mut color)?;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let v = opart(x, y, w, h);
                for c in 0..3 {
                    px[i + c] = (dark[c] + v * (cream[c] - dark[c])).clamp(0.0, 255.0) as u8;
                }
                // leave alpha as-is
            }
        }
    }
    let new_color = morphic::replace_mip_chain(&color_bytes, &color)?;
    eprintln!("albedo re-encoded ({w}x{h})");

    // --- normal+roughness: flat normal, op-art gloss in B ---
    let mut nr = morphic::decode(&nr_donor_bytes)?;
    let (nw, nh) = (nr.width, nr.height);
    {
        let px = rgba8_mut(&mut nr)?;
        for y in 0..nh {
            for x in 0..nw {
                let i = ((y * nw + x) * 4) as usize;
                let v = opart(x, y, nw, nh);
                // Frostline's proven in-game recipe: uniformly LOW roughness reads as
                // shiny/metallic under NPR (its flat 0.188 "just worked"). A glossy-to-matte
                // swing does NOT read (the matte half dominates and the high-freq pattern
                // mip-averages to mid-gray). So keep the WHOLE range glossy: cream band
                // B=20 (rough 0.08), dark band B=52 (rough 0.20, under the 0.188 that worked).
                // The whole dress stays satin even mip-averaged; cream bands catch extra glint.
                let rough = (52.0 - v * 32.0).clamp(0.0, 255.0) as u8;
                px[i] = 128; // normal X neutral
                px[i + 1] = 128; // normal Y neutral
                px[i + 2] = rough; // roughness
                px[i + 3] = 255;
            }
        }
    }
    let new_nr = morphic::replace_mip_chain(&nr_donor_bytes, &nr)?;
    eprintln!("normal+roughness re-encoded ({nw}x{nh})");

    let readme = format!(
        "Vindicta Op-Art Dress\n\
         =====================\n\
         Optical-illusion reskin (vpkmerge test build).\n\
         Hero: Vindicta (hornet)\n\
         Material: vindicta_dress.vmat\n\
         Edits: g_tColor + g_tNormalRoughness overridden with a tiling op-art\n\
         wave pattern (cream vs deep maroon); roughness kept uniformly glossy\n\
         (~0.08-0.20, Frostline's proven metallic recipe) so the dress reads satin.\n\
         No material/.vmat_c edit; pure texture override.\n\
         Built by vpkmerge reskin_dress example.\n"
    );

    vpkmerge_core::pack(
        &[
            (COLOR, new_color.as_slice()),
            (NORMALROUGH, new_nr.as_slice()),
            ("README.txt", readme.as_bytes()),
        ],
        &out,
    )?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
