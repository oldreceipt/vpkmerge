// Build a Vindicta optical-illusion skin into one addon VPK.
//   dress g_tColor           -> opart | cafewall | wetlens | mirrorlens | shatterbloom
//   dress g_tNormalRoughness -> glossy/wet roughness or raised crack normals
//   gun   g_tColor           -> cheetah-print accents on the wooden parts
//   gun   g_tNormalRoughness -> glossy
//
// usage:
//   cargo run --release -p vpkmerge-core --example reskin_skin -- \
//     <pak01_dir.vpk> <out_dir.vpk> [opart|cafewall|wetlens|mirrorlens|shatterbloom]
use morphic::{Image, ImageData};

const DRESS_COLOR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_color_png_a192a2cd.vtex_c";
const DRESS_NR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_vmat_g_tnormalroughness_ce38f34.vtex_c";
const DRESS_NR_DONOR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_dress_normal_png_f4a1a6e6.vtex_c";
const GUN_COLOR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_gun_color_png_62f46a62.vtex_c";
const GUN_NR: &str =
    "models/heroes_staging/vindicta/materials/vindicta_gun_vmat_g_tnormalroughness_c0a19034.vtex_c";

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add_scaled(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        (a[0] + b[0] * t).clamp(0.0, 255.0),
        (a[1] + b[1] * t).clamp(0.0, 255.0),
        (a[2] + b[2] * t).clamp(0.0, 255.0),
    ]
}

fn is_lens_pattern(pattern: &str) -> bool {
    matches!(pattern, "wetlens" | "mirrorlens")
}

fn is_relief_pattern(pattern: &str) -> bool {
    is_lens_pattern(pattern) || pattern == "shatterbloom"
}

fn opart_v(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let (stripes, wave_m, amp) = (22.0_f32, 6.0_f32, 0.42_f32);
    let tau = std::f32::consts::TAU;
    let u = x as f32 / w as f32;
    let vv = y as f32 / h as f32;
    let disp = amp * (tau * wave_m * vv).sin();
    smoothstep(-0.12, 0.12, (tau * (u * stripes + disp)).sin())
}

fn cafewall(x: u32, y: u32, w: u32, h: u32) -> [f32; 3] {
    let (cols, rows) = (12.0_f32, 18.0_f32);
    let bw = w as f32 / cols;
    let bh = h as f32 / rows;
    let mortar_t = bh * 0.05;
    let yy = y as f32;
    let row = (yy / bh).floor();
    let in_row = yy - row * bh;
    if in_row.min(bh - in_row) < mortar_t {
        return [120.0, 118.0, 122.0];
    }
    let offset = if (row as i32) % 2 == 0 { 0.0 } else { bw * 0.5 };
    let bx = ((x as f32 + offset) / bw).floor() as i32;
    if bx.rem_euclid(2) == 0 {
        [26.0, 22.0, 28.0]
    } else {
        [230.0, 228.0, 222.0]
    }
}

fn wetlens_fields(x: u32, y: u32, w: u32, h: u32) -> (f32, f32, f32) {
    let tau = std::f32::consts::TAU;
    let u = x as f32 / w as f32;
    let v = y as f32 / h as f32;

    let warp_u = 0.038 * (tau * (v * 3.0 + 0.18 * (tau * u * 2.0).sin())).sin()
        + 0.016 * (tau * (u * 5.0 + v * 2.0)).sin();
    let warp_v = 0.030 * (tau * (u * 4.0 - 0.20 * (tau * v * 3.0).sin())).sin()
        + 0.014 * (tau * (v * 6.0 - u)).sin();
    let ru = u + warp_u;
    let rv = v + warp_v;

    let a = (tau * (ru * 17.0 + 0.36 * (tau * rv * 5.0).sin())).sin();
    let b = (tau * ((ru * 0.78 + rv * 0.62) * 19.0 + 0.28 * (tau * ((ru - rv) * 4.0)).sin())).sin();
    let c =
        (tau * ((ru * 0.31 - rv * 0.95) * 13.0 + 0.16 * (tau * (ru * 7.0 + rv * 2.0)).sin())).sin();

    let lens_u = u * 4.0;
    let lens_v = v * 4.0;
    let fu = lens_u - lens_u.floor() - 0.5;
    let fv = lens_v - lens_v.floor() - 0.5;
    let r = ((fu * 1.15).powi(2) + (fv * 0.82).powi(2)).sqrt();
    let lens = smoothstep(0.45, 0.11, r);
    let ring = 1.0 - smoothstep(0.018, 0.070, (r - 0.31).abs());
    let ripple = (tau * (r * 8.0 - fu.atan2(fv) * 0.18)).sin();

    let interference = a * 0.58 + b * 0.42 + c * 0.24 + lens * ripple * 0.38;
    let band = smoothstep(-0.10, 0.10, interference);
    let caustic_wave = (tau * (ru * 11.0 - rv * 13.0 + lens * 0.75)).sin();
    let caustic = smoothstep(0.70, 0.97, caustic_wave) * (0.35 + lens * 0.65);
    (band, caustic, ring)
}

fn wetlens_height(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let (band, caustic, ring) = wetlens_fields(x, y, w, h);
    band * 0.55 + caustic * 0.35 + ring * 0.28
}

fn wrapped_delta(a: f32, b: f32, period: f32) -> f32 {
    let d = (a - b).abs();
    d.min(period - d)
}

fn shatterbloom_fields(x: u32, y: u32, w: u32, h: u32) -> (f32, f32, f32, f32) {
    let cells = 15_i32;
    let u = x as f32 / w as f32 * cells as f32;
    let v = y as f32 / h as f32 * cells as f32;
    let cx = u.floor() as i32;
    let cy = v.floor() as i32;

    let mut d1 = f32::INFINITY;
    let mut d2 = f32::INFINITY;
    let mut nearest = (0_i32, 0_i32);
    let mut local = (0.0_f32, 0.0_f32);

    for dy in -1..=1 {
        for dx in -1..=1 {
            let gx = cx + dx;
            let gy = cy + dy;
            let wx = gx.rem_euclid(cells);
            let wy = gy.rem_euclid(cells);
            let jx = 0.5 + (hash01(wx + 11, wy + 29) - 0.5) * 0.58;
            let jy = 0.5 + (hash01(wx + 67, wy + 43) - 0.5) * 0.58;
            let px = gx as f32 + jx;
            let py = gy as f32 + jy;
            let ddx = wrapped_delta(u, px, cells as f32);
            let ddy = wrapped_delta(v, py, cells as f32);
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            if dist < d1 {
                d2 = d1;
                d1 = dist;
                nearest = (wx, wy);
                local = (u - px, v - py);
            } else if dist < d2 {
                d2 = dist;
            }
        }
    }

    let seed = hash01(nearest.0 + 101, nearest.1 + 197);
    let phase = hash01(nearest.0 + 313, nearest.1 + 71) * std::f32::consts::TAU;
    let theta = local.1.atan2(local.0);
    let petal = (theta * 5.0 + phase).sin() * 0.045 + (theta * 9.0 - phase * 0.7).sin() * 0.020;
    let radius = 0.20 + seed * 0.10 + petal * 0.75;
    let presence = smoothstep(0.46, 0.86, seed);
    let shard = smoothstep(radius + 0.060, radius - 0.030, d1) * presence;

    let gap = d2 - d1;
    let crack = smoothstep(0.060, 0.010, gap);
    let crack_core = smoothstep(0.020, 0.003, gap);
    let enamel_wave =
        0.5 + 0.5 * ((u * 0.71 + v * 0.37 + (u * 1.6).sin() * 0.09) * std::f32::consts::TAU).sin();
    (shard, crack, crack_core, enamel_wave)
}

fn shatterbloom_height(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let (shard, crack, crack_core, enamel_wave) = shatterbloom_fields(x, y, w, h);
    shard * 0.18 + crack * 0.42 + crack_core * 0.60 + enamel_wave * 0.035
}

fn lens_rgb(pattern: &str, x: u32, y: u32, w: u32, h: u32) -> [f32; 3] {
    let (band, caustic, ring) = wetlens_fields(x, y, w, h);
    let ink = [4.0_f32, 6.0, 9.0];
    let deep_oil = [12.0_f32, 20.0, 31.0];

    if pattern == "mirrorlens" {
        let graphite = [42.0_f32, 44.0, 47.0];
        let pearl = [214.0_f32, 224.0, 228.0];
        let blue_sheen = [38.0_f32, 112.0, 158.0];
        let base = mix(ink, deep_oil, 0.45 + 0.25 * band);
        let banded = mix(base, pearl, band.powf(1.35) * 0.74);
        let shadowed = mix(banded, graphite, (1.0 - band).powf(1.8) * 0.42);
        let with_sheen = add_scaled(shadowed, blue_sheen, caustic * 0.33);
        return add_scaled(
            with_sheen,
            pearl,
            (caustic * 0.42 + ring * 0.30).clamp(0.0, 0.72),
        );
    }

    let graphite = [34.0_f32, 38.0, 43.0];
    let pearl = [190.0_f32, 208.0, 216.0];
    let blue_sheen = [22.0_f32, 82.0, 122.0];
    let mid_band = smoothstep(0.36, 0.68, band) * 0.24;
    let bright_ribbon = smoothstep(0.66, 0.92, band);
    let base = mix(ink, deep_oil, 0.55 + 0.18 * band);
    let smoky = mix(base, graphite, mid_band);
    let banded = mix(smoky, pearl, bright_ribbon * 0.46);
    let with_sheen = add_scaled(banded, blue_sheen, caustic * 0.22);
    add_scaled(
        with_sheen,
        pearl,
        (caustic * 0.16 + ring * 0.18).clamp(0.0, 0.34),
    )
}

fn dress_rgb(pattern: &str, x: u32, y: u32, w: u32, h: u32) -> [f32; 3] {
    match pattern {
        "cafewall" => cafewall(x, y, w, h),
        "wetlens" | "mirrorlens" => lens_rgb(pattern, x, y, w, h),
        "shatterbloom" => {
            let (shard, crack, crack_core, enamel_wave) = shatterbloom_fields(x, y, w, h);
            let obsidian = [10.0_f32, 7.0, 13.0];
            let oxblood = [58.0_f32, 12.0, 28.0];
            let bruised_plum = [82.0_f32, 31.0, 55.0];
            let ivory = [225.0_f32, 211.0, 184.0];
            let blush = [206.0_f32, 157.0, 139.0];
            let antique_gold = [211.0_f32, 151.0, 55.0];
            let hot_gold = [255.0_f32, 220.0, 112.0];

            let lacquer = mix(obsidian, oxblood, 0.28 + enamel_wave * 0.36);
            let lacquer = mix(lacquer, bruised_plum, enamel_wave.powf(2.0) * 0.26);
            let porcelain = mix(ivory, blush, 0.22 + enamel_wave * 0.28);
            let with_shards = mix(lacquer, porcelain, shard * 0.88);
            let with_gold = mix(with_shards, antique_gold, crack * 0.82);
            add_scaled(with_gold, hot_gold, crack_core * 0.42)
        }
        _ => {
            let v = opart_v(x, y, w, h);
            let cream = [233.0_f32, 224.0, 206.0];
            let dark = [30.0_f32, 13.0, 18.0];
            [
                dark[0] + v * (cream[0] - dark[0]),
                dark[1] + v * (cream[1] - dark[1]),
                dark[2] + v * (cream[2] - dark[2]),
            ]
        }
    }
}

fn dress_rough(pattern: &str, x: u32, y: u32, w: u32, h: u32) -> u8 {
    match pattern {
        "cafewall" => 36,
        "wetlens" | "mirrorlens" => {
            let (band, caustic, ring) = wetlens_fields(x, y, w, h);
            let wet = 26.0 - caustic * 12.0 - ring * 5.0 + (1.0 - band) * 6.0;
            wet.clamp(10.0, 34.0) as u8
        }
        "shatterbloom" => {
            let (shard, crack, crack_core, _) = shatterbloom_fields(x, y, w, h);
            let rough = 145.0 + shard * 22.0 - crack * 82.0 - crack_core * 26.0;
            rough.clamp(46.0, 176.0) as u8
        }
        _ => {
            let v = opart_v(x, y, w, h);
            (52.0 - v * 32.0).clamp(0.0, 255.0) as u8
        }
    }
}

fn dress_normalrough_rgba(pattern: &str, x: u32, y: u32, w: u32, h: u32) -> [u8; 4] {
    if !is_relief_pattern(pattern) {
        return [128, 128, dress_rough(pattern, x, y, w, h), 255];
    }

    let xl = if x == 0 { w - 1 } else { x - 1 };
    let xr = if x + 1 == w { 0 } else { x + 1 };
    let yu = if y == 0 { h - 1 } else { y - 1 };
    let yd = if y + 1 == h { 0 } else { y + 1 };
    let height = if pattern == "shatterbloom" {
        shatterbloom_height
    } else {
        wetlens_height
    };
    let strength = if pattern == "shatterbloom" {
        58.0
    } else {
        40.0
    };
    let dx = height(xr, y, w, h) - height(xl, y, w, h);
    let dy = height(x, yd, w, h) - height(x, yu, w, h);
    let nx = (128.0 - dx * strength).clamp(92.0, 164.0) as u8;
    let ny = (128.0 - dy * strength).clamp(92.0, 164.0) as u8;
    let rough = dress_rough(pattern, x, y, w, h);
    [nx, ny, rough, rough]
}

fn hash01(a: i32, b: i32) -> f32 {
    let mut h = (a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))) as u32;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

fn cheetah(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let n = 27_i32;
    let u = x as f32 / w as f32 * n as f32;
    let v = y as f32 / h as f32 * n as f32;
    let cx = u.floor() as i32;
    let cy = v.floor() as i32;
    let mut s = 0.0_f32;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cx + dx, cy + dy);
            let (wx, wy) = (gx.rem_euclid(n), gy.rem_euclid(n));
            if hash01(wx + 41, wy + 23) < 0.05 {
                continue;
            }
            let jx = 0.5 + (hash01(wx, wy) - 0.5) * 0.8;
            let jy = 0.5 + (hash01(wx + 97, wy + 131) - 0.5) * 0.8;
            let ddx = u - (gx as f32 + jx);
            let ddy = v - (gy as f32 + jy);
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            let theta = ddy.atan2(ddx);
            let base_r = 0.18 + 0.20 * hash01(wx + 13, wy + 57);
            let ph = hash01(wx + 7, wy + 3) * 6.283;
            let r = base_r
                * (1.0 + 0.12 * (2.0 * theta + ph).sin() + 0.05 * (5.0 * theta + ph * 1.7).sin());
            if dist < r {
                s = s.max(smoothstep(r, r * 0.7, dist));
            }
        }
    }
    s
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
    let pattern = a.next().unwrap_or_else(|| "opart".to_string());

    let dress_color_bytes = vpkmerge_core::read_vpk_entry(&pak, DRESS_COLOR)?;
    let dress_nr_donor = vpkmerge_core::read_vpk_entry(&pak, DRESS_NR_DONOR)?;

    let mut color = morphic::decode(&dress_color_bytes)?;
    let (w, h) = (color.width, color.height);
    {
        let px = rgba8_mut(&mut color)?;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let rgb = dress_rgb(&pattern, x, y, w, h);
                for c in 0..3 {
                    px[i + c] = rgb[c].clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    let new_dress_color = morphic::replace_mip_chain(&dress_color_bytes, &color)?;

    let mut nr = morphic::decode(&dress_nr_donor)?;
    let (nw, nh) = (nr.width, nr.height);
    {
        let px = rgba8_mut(&mut nr)?;
        for y in 0..nh {
            for x in 0..nw {
                let i = ((y * nw + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&dress_normalrough_rgba(&pattern, x, y, nw, nh));
            }
        }
    }
    let new_dress_nr = morphic::replace_mip_chain(&dress_nr_donor, &nr)?;
    eprintln!("dress: '{pattern}' albedo + wet normalroughness ({w}x{h})");

    let gun_bytes = vpkmerge_core::read_vpk_entry(&pak, GUN_COLOR)?;
    let mut gun = morphic::decode(&gun_bytes)?;
    let (gw, gh) = (gun.width, gun.height);
    let tan = [202.0_f32, 162.0, 92.0];
    let spot = [26.0_f32, 17.0, 11.0];
    {
        let px = rgba8_mut(&mut gun)?;
        for y in 0..gh {
            for x in 0..gw {
                let i = ((y * gw + x) * 4) as usize;
                let (or, og, ob) = (px[i] as f32, px[i + 1] as f32, px[i + 2] as f32);
                let lum = 0.299 * or + 0.587 * og + 0.114 * ob;
                let wood = smoothstep(25.0, 52.0, lum);
                let sp = cheetah(x, y, gw, gh);
                let orig = [or, og, ob];
                for c in 0..3 {
                    let cheetah_col = spot[c] + (1.0 - sp) * (tan[c] - spot[c]);
                    px[i + c] =
                        (orig[c] * (1.0 - wood) + cheetah_col * wood).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    let new_gun_color = morphic::replace_mip_chain(&gun_bytes, &gun)?;

    let mut gnr = morphic::decode(&gun_bytes)?;
    {
        let px = rgba8_mut(&mut gnr)?;
        for p in px.chunks_mut(4) {
            p[0] = 128;
            p[1] = 128;
            p[2] = 72;
            p[3] = 72;
        }
    }
    let new_gun_nr = morphic::replace_mip_chain(&gun_bytes, &gnr)?;
    eprintln!("gun: cheetah-on-wood albedo + glossy roughness ({gw}x{gh})");

    let readme = format!(
        "Vindicta Illusion Skin ({pattern} dress + cheetah gun)\n\
         Built by vpkmerge reskin_skin.rs. Pure texture override, no material edits.\n\
         Dress: tiling '{pattern}' illusion with low roughness and ripple normals.\n\
         Gun: cheetah-print accents on the wooden parts, glossy.\n"
    );

    vpkmerge_core::pack(
        &[
            (DRESS_COLOR, new_dress_color.as_slice()),
            (DRESS_NR, new_dress_nr.as_slice()),
            (GUN_COLOR, new_gun_color.as_slice()),
            (GUN_NR, new_gun_nr.as_slice()),
            ("README.txt", readme.as_bytes()),
        ],
        &out,
    )?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
