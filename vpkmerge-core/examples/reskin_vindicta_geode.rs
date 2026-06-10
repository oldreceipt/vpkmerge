// Build the Vindicta "blue agate geode" reskin and pack it into an addon VPK.
//
// Pure texture override on `vindicta_dress.vmat` (pbr.vfx, F_USE_NPR_LIGHTING) -- no
// .vmat_c edit. Drives every channel the material already samples to read as a real
// cut gem with reflection + depth, not a flat painted crystal:
//   g_tColor              -> seamless agate banding + druzy crystal facets (teal/sky/pale)
//   g_tNormalRoughness    -> per-facet tilted NORMAL (RG) so each crystal face reflects the
//                            environment from its own angle, + uniformly-low ROUGHNESS (B)
//                            for sharp gem reflections
//   g_tAmbientOcclusion   -> dark crystal seams (contact-shadow depth in the crevices)
//   g_tNPRTransmissiveColor -> icy-cyan translucency so light bleeds THROUGH the crystal
//   g_tSelfIllumMask      -> faint glow at the druzy points (backlit-crystal sparkle)
// All tiling/seamless so it survives the dress mesh's scrambled, overlapping UVs.
//
// usage:
//   cargo run --release --example reskin_vindicta_geode -- --preview <dir>            # dump per-channel PNGs
//   cargo run --release --example reskin_vindicta_geode -- <pak01_dir.vpk> <out_dir.vpk>  # bake VPK
use morphic::{Image, ImageData, TextureFormat};

const DIR: &str = "models/heroes_staging/vindicta/materials/";
const COLOR: &str = "vindicta_dress_color_png_a192a2cd.vtex_c";
const NORMALROUGH: &str = "vindicta_dress_vmat_g_tnormalroughness_ce38f34.vtex_c";
const AO: &str = "vindicta_dress_ao_png_c8cd108a.vtex_c";
const TRANSMISSIVE: &str = "vindicta_dress_vmat_g_tnprtransmissivecolor_bf71c723.vtex_c";
const SELFILLUM: &str = "vindicta_dress_vmat_g_tselfillummask_2b2ebb3f.vtex_c";
// 2048^2 BC7 normal texture reused only as the encode container for the normal+roughness map
// (the op-art dress reskin proved this packs and loads in-game).
const NR_DONOR: &str = "vindicta_dress_normal_png_f4a1a6e6.vtex_c";

// gun (color + normalroughness + AO + rim mask; no transmissive/self-illum)
const GUN_COLOR: &str = "vindicta_gun_color_png_62f46a62.vtex_c";
const GUN_NR: &str = "vindicta_gun_vmat_g_tnormalroughness_c0a19034.vtex_c";
const GUN_AO: &str = "vindicta_gun_ao_png_fc1a1d63.vtex_c";

// props (color + normalroughness + AO)
const PROPS_COLOR: &str = "vindicta_props_color_png_c9c851e8.vtex_c";
const PROPS_NR: &str = "vindicta_props_vmat_g_tnormalroughness_58ee725f.vtex_c";
const PROPS_AO: &str = "vindicta_props_ao_png_30029314.vtex_c";

// --- agate palette (blue/teal) ---
const TEAL: [f32; 3] = [11.0, 61.0, 92.0]; // #0b3d5c deep band
const SKY: [f32; 3] = [46.0, 139.0, 192.0]; // #2e8bc0 mid band
const PALE: [f32; 3] = [174.0, 227.0, 245.0]; // #aee3f5 light band
const QUARTZ: [f32; 3] = [242.0, 251.0, 255.0]; // #f2fbff druzy core / sparkle

const TAU: f32 = std::f32::consts::TAU;
const FACET_TILT: f32 = 0.62; // max facet tilt in radians (~35 deg)

// Per-material crystal scale: (big-facet grid, fine-druzy grid).
const DRESS_SCALE: (i32, i32) = (14, 30);
const GUN_SCALE: (i32, i32) = (22, 44); // smaller crystals on a thin weapon
const PROPS_SCALE: (i32, i32) = (20, 40);

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
}

fn hash21(ix: i32, iy: i32) -> f32 {
    let mut h = (ix.wrapping_mul(374_761_393)).wrapping_add(iy.wrapping_mul(668_265_263)) as u32;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}
fn hash2(ix: i32, iy: i32, salt: i32) -> f32 {
    hash21(ix.wrapping_add(salt.wrapping_mul(101)), iy.wrapping_sub(salt.wrapping_mul(57)))
}

// three-stop agate ramp teal -> sky -> pale -> sky (palindrome, seamless), phase in [0,1)
fn agate_color(phase: f32) -> [f32; 3] {
    let p = phase.rem_euclid(1.0) * 4.0;
    let seg = p.floor() as i32 % 4;
    let f = p.fract();
    let stops = [TEAL, SKY, PALE, SKY];
    lerp3(stops[seg as usize], stops[((seg + 1) % 4) as usize], smoothstep(0.0, 1.0, f))
}

// Periodic Voronoi over a GRID x GRID torus.
// Returns (f1, f2, winning-cell wrapped coords).
fn voronoi(u: f32, v: f32, grid: i32) -> (f32, f32, i32, i32) {
    let g = grid as f32;
    let gx = (u * g).floor() as i32;
    let gy = (v * g).floor() as i32;
    let fx = u * g - gx as f32;
    let fy = v * g - gy as f32;
    let (mut f1, mut f2) = (9.0f32, 9.0f32);
    let (mut cwx, mut cwy) = (0, 0);
    for oy in -1..=1 {
        for ox in -1..=1 {
            let (wx, wy) = ((gx + ox).rem_euclid(grid), (gy + oy).rem_euclid(grid));
            let px = ox as f32 + hash2(wx, wy, 1) - fx;
            let py = oy as f32 + hash2(wx, wy, 2) - fy;
            let d = (px * px + py * py).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                cwx = wx;
                cwy = wy;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, cwx, cwy)
}

struct Gem {
    col: [f32; 3],
    nx: f32, // tangent-space normal X/Y in [-1,1]
    ny: f32,
    rough: f32,       // 0..1
    ao: f32,          // 0..1 (1 = lit)
    glow: f32,        // 0..1 self-illum mask
    trans: [f32; 3],  // transmissive color bytes 0..255
}

fn gem(u: f32, v: f32, scale: (i32, i32)) -> Gem {
    // wavy agate bands (integer counts -> seamless both axes)
    let disp = 0.18 * (TAU * 3.0 * v).sin();
    let band_phase = u * 5.0 + disp + 0.12 * (TAU * 2.0 * v).sin();

    let (f1a, f2a, cwx, cwy) = voronoi(u, v, scale.0);
    let (f1b, f2b, _, _) = voronoi(u + 0.37, v + 0.19, scale.1);

    // per-cell hashes drive both color band and facet orientation
    let cell_h = hash2(cwx, cwy, 7);
    let cell_phase = band_phase + (cell_h - 0.5) * 0.9;
    let mut col = agate_color(cell_phase);

    // facet light: brighten toward each cell's seed
    let facet_lit = (1.0 - (f1a / 0.085).min(1.0)).powf(1.5);
    col = lerp3(col, lerp3(col, QUARTZ, 0.55), 0.35 * facet_lit);
    let trough = smoothstep(0.0, 0.12, f1a);
    col = lerp3(col, [col[0] * 0.62, col[1] * 0.62, col[2] * 0.72], 0.30 * trough);

    // druzy seams (bright crystal borders) feed both albedo sparkle and self-illum
    let seam_a = 1.0 - smoothstep(0.0, 0.035, f2a - f1a);
    let seam_b = 1.0 - smoothstep(0.0, 0.020, f2b - f1b);
    let seam = (seam_a.max(seam_b * 0.8)).min(1.0);
    col = lerp3(col, QUARTZ, 0.85 * seam);
    let druzy = cell_h > 0.93;
    if druzy {
        col = lerp3(col, QUARTZ, 0.6);
    }

    // --- faceted normal: each crystal face tilts in a fixed per-cell direction ---
    // azimuth + tilt from cell hashes; constant across the cell with hard seams = cut facets.
    let phi = hash2(cwx, cwy, 3) * TAU;
    let theta = (0.35 + 0.65 * hash2(cwx, cwy, 4)) * FACET_TILT;
    let (mut nx, mut ny) = (theta.sin() * phi.cos(), theta.sin() * phi.sin());
    // soften the very center of each facet slightly flatter (rounded crystal top)
    let center = 1.0 - smoothstep(0.0, 0.05, f1a);
    nx *= 1.0 - 0.4 * center;
    ny *= 1.0 - 0.4 * center;

    // --- roughness: uniformly glossy gem; seams a touch sharper for glints ---
    let rough = (0.14 - 0.08 * seam).clamp(0.03, 1.0);

    // --- AO: dark in the crystal crevices (seams + cell troughs) ---
    let ao = (1.0 - 0.7 * seam) * (0.85 + 0.15 * (1.0 - trough));

    // --- self-illum: faint glow at druzy points only (base mask is black; stay conservative) ---
    let glow = if druzy { 0.55 } else { 0.10 * facet_lit + 0.25 * seam_b };

    // --- transmissive: icy cyan, brighter where the facet is lit (thin/translucent reads) ---
    let trans_base = [70.0, 150.0, 185.0];
    let trans_bright = [150.0, 220.0, 245.0];
    let trans = lerp3(trans_base, trans_bright, 0.4 + 0.6 * facet_lit);

    Gem { col, nx, ny, rough, ao, glow, trans }
}

fn rgba8_mut(img: &mut Image) -> anyhow::Result<&mut Vec<u8>> {
    match &mut img.data {
        ImageData::Rgba8(v) => Ok(v),
        ImageData::Rgba16F(_) => anyhow::bail!("unexpected HDR texture"),
    }
}

// Paint one channel-set into a decoded donor image at its own resolution, re-encode in place.
fn bake_channel(
    src_bytes: &[u8],
    scale: (i32, i32),
    f: impl Fn(&Gem) -> [u8; 4],
) -> anyhow::Result<Vec<u8>> {
    let mut img = morphic::decode(src_bytes)?;
    let (w, h) = (img.width, img.height);
    {
        let px = rgba8_mut(&mut img)?;
        for y in 0..h {
            for x in 0..w {
                let g = gem(x as f32 / w as f32, y as f32 / h as f32, scale);
                let o = f(&g);
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&o);
            }
        }
    }
    Ok(morphic::replace_mip_chain(src_bytes, &img)?)
}

fn nbyte(n: f32) -> u8 {
    (128.0 + n.clamp(-1.0, 1.0) * 127.0).clamp(0.0, 255.0) as u8
}

fn write_png(path: &str, w: u32, h: u32, f: impl Fn(&Gem) -> [u8; 4]) -> anyhow::Result<()> {
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let g = gem(x as f32 / w as f32, y as f32 / h as f32, DRESS_SCALE);
            let o = f(&g);
            let i = ((y * w + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&o);
        }
    }
    let img = Image { width: w, height: h, data: ImageData::Rgba8(buf) };
    std::fs::write(path, morphic::encode_image(&img, TextureFormat::PngRgba8888)?)?;
    eprintln!("wrote {path} ({w}x{h})");
    Ok(())
}

// channel encoders ---------------------------------------------------------
fn enc_color(g: &Gem) -> [u8; 4] {
    [g.col[0] as u8, g.col[1] as u8, g.col[2] as u8, 255]
}
fn enc_nr(g: &Gem) -> [u8; 4] {
    [nbyte(g.nx), nbyte(g.ny), (g.rough * 255.0) as u8, 255]
}
fn enc_normal_rgb(g: &Gem) -> [u8; 4] {
    // OpenGL-style RGB normal for Blender preview (B = +Z)
    let nz = (1.0 - (g.nx * g.nx + g.ny * g.ny)).max(0.0).sqrt();
    [nbyte(g.nx), nbyte(g.ny), (nz * 255.0) as u8, 255]
}
fn enc_ao(g: &Gem) -> [u8; 4] {
    let a = (g.ao * 255.0) as u8;
    [a, a, a, 255]
}
fn enc_glow(g: &Gem) -> [u8; 4] {
    let a = (g.glow * 255.0) as u8;
    [a, a, a, 255]
}
fn enc_rough(g: &Gem) -> [u8; 4] {
    let r = (g.rough * 255.0) as u8;
    [r, r, r, 255]
}
fn enc_trans(g: &Gem) -> [u8; 4] {
    [g.trans[0] as u8, g.trans[1] as u8, g.trans[2] as u8, 255]
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let first = a.next().expect("need <pak> <out>  OR  --preview <dir>");
    if first == "--preview" {
        let dir = a.next().expect("--preview needs a dir");
        std::fs::create_dir_all(&dir)?;
        let p = |n: &str| format!("{dir}/{n}");
        write_png(&p("albedo.png"), 1024, 1024, enc_color)?;
        write_png(&p("normal.png"), 1024, 1024, enc_normal_rgb)?;
        write_png(&p("roughness.png"), 1024, 1024, enc_rough)?;
        write_png(&p("ao.png"), 1024, 1024, enc_ao)?;
        write_png(&p("glow.png"), 1024, 1024, enc_glow)?;
        write_png(&p("transmissive.png"), 1024, 1024, enc_trans)?;
        return Ok(());
    }
    let pak = first;
    let out = a.next().expect("out_dir.vpk");
    let read = |name: &str| vpkmerge_core::read_vpk_entry(&pak, &format!("{DIR}{name}"));
    let entry = |name: &str| format!("{DIR}{name}");

    // accumulate (entry, bytes) overrides
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    // --- dress: full gem set (the showcase) ---
    let d = DRESS_SCALE;
    files.push((entry(COLOR), bake_channel(&read(COLOR)?, d, enc_color)?));
    // paint NR into the BC7 donor, pack at the real NORMALROUGH path
    files.push((entry(NORMALROUGH), bake_channel(&read(NR_DONOR)?, d, enc_nr)?));
    files.push((entry(AO), bake_channel(&read(AO)?, d, enc_ao)?));
    files.push((entry(TRANSMISSIVE), bake_channel(&read(TRANSMISSIVE)?, d, enc_trans)?));
    files.push((entry(SELFILLUM), bake_channel(&read(SELFILLUM)?, d, enc_glow)?));
    eprintln!("dress done (5 channels)");

    // --- gun: agate + faceted normal/roughness + AO ---
    let g = GUN_SCALE;
    files.push((entry(GUN_COLOR), bake_channel(&read(GUN_COLOR)?, g, enc_color)?));
    files.push((entry(GUN_NR), bake_channel(&read(GUN_NR)?, g, enc_nr)?));
    files.push((entry(GUN_AO), bake_channel(&read(GUN_AO)?, g, enc_ao)?));
    eprintln!("gun done (3 channels)");

    // --- props: agate + faceted normal/roughness + AO ---
    let p = PROPS_SCALE;
    files.push((entry(PROPS_COLOR), bake_channel(&read(PROPS_COLOR)?, p, enc_color)?));
    files.push((entry(PROPS_NR), bake_channel(&read(PROPS_NR)?, p, enc_nr)?));
    files.push((entry(PROPS_AO), bake_channel(&read(PROPS_AO)?, p, enc_ao)?));
    eprintln!("props done (3 channels)");

    let readme = "Vindicta Blue Agate Geode\n\
         =========================\n\
         Gem/geode reskin (vpkmerge test build).  Hero: Vindicta (hornet)\n\
         Materials: vindicta_dress / _gun / _props (all pbr.vfx NPR).\n\
         Pure texture override, no .vmat_c edit.\n\
         Dress: g_tColor (agate + druzy facets) + g_tNormalRoughness (faceted normals +\n\
         low gem roughness) + g_tAmbientOcclusion (seam depth) + g_tNPRTransmissiveColor\n\
         (icy translucency) + g_tSelfIllumMask (druzy glow).\n\
         Gun + props: agate color + faceted normal/roughness + seam AO.\n\
         Built by vpkmerge reskin_vindicta_geode example.\n";
    files.push(("README.txt".to_string(), readme.as_bytes().to_vec()));

    let refs: Vec<(&str, &[u8])> = files.iter().map(|(k, v)| (k.as_str(), v.as_slice())).collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!("wrote addon VPK: {out}  ({} files)", refs.len());
    Ok(())
}
