// Build Paradox's "trippy" animated skin and pack it into one addon VPK.
//
// Mesh-side analog of the Yamato animated particle prism: a tiling acid-confetti
// RAINBOW albedo plus pbr.vfx UV-scroll so the color FLOWS at runtime. All vmat
// edits are byte-faithful IN-PLACE double patches (set_doubles) -- a full
// `encode_kv3_resource` re-serialize renders as the engine error shader in-game.
//
// Covers:
//   body  -> rainbow albedo + g_vAlbedoScrollSpeed1 + g_vSelfIllumScrollSpeed1
//   gun   -> rainbow albedo packed over its flat g_tColor placeholder, tint nulled,
//            + albedo scroll (best-effort)
//   head  -> glass dome brightened (TextureColor1 ~black -> light) so it reads CLEAR
//   hgls  -> hourglass self-illum glow retinted crimson -> neon cyan
//
// usage:
//   cargo run --release --example reskin_chrono_trippy -- <pak01_dir.vpk> <out_dir.vpk>
//   cargo run --release --example reskin_chrono_trippy -- <pak01_dir.vpk> --png <preview.png>
use morphic::kv3::{Seg, Value};
use morphic::{Image, ImageData, TextureFormat};
use std::f32::consts::TAU;

const BODY_COLOR: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_color_png_d1d22ba7.vtex_c";
const BODY_VMAT: &str = "models/heroes_staging/chrono/materials/chrono_v2.vmat_c";
// Gun g_tColor is a flat 4x4 placeholder; we override it in place with a full-res
// rainbow (no .vmat repoint needed -- the material already points at this path).
const GUN_COLOR: &str =
    "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun_vmat_g_tcolor_7d4419c1.vtex_c";
const GUN_VMAT: &str = "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun.vmat_c";
const HEADGLASS_VMAT: &str = "models/heroes_staging/chrono/materials/chrono_v2_headglass.vmat_c";
const HOURGLASS_VMAT: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_head_hourglass.vmat_c";

// UV units/sec. The albedo packs many confetti cells per tile, so even a small
// scroll moves a lot of cells visually. Tunable.
const SCROLL: [f64; 4] = [0.08, 0.05, 0.0, 0.0];

// ---------------------------------------------------------------------------
// Tiling acid-confetti rainbow generator. Every basis function is period-1 in
// both u and v (integer-lattice value noise wrapped mod P), so the whole field
// tiles seamlessly and the runtime UV scroll wraps without a seam.
// ---------------------------------------------------------------------------

fn hash2(i: i64, j: i64) -> f32 {
    let mut h = (i
        .wrapping_mul(374_761_393)
        .wrapping_add(j.wrapping_mul(668_265_263))) as u64;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xff_ffff) as f32 / 16_777_216.0
}

fn vnoise(x: f32, y: f32, p: i64) -> f32 {
    let gx = x * p as f32;
    let gy = y * p as f32;
    let x0 = gx.floor() as i64;
    let y0 = gy.floor() as i64;
    let fx = gx - x0 as f32;
    let fy = gy - y0 as f32;
    let wrap = |a: i64| ((a % p) + p) % p;
    let (x0w, y0w) = (wrap(x0), wrap(y0));
    let (x1w, y1w) = (wrap(x0 + 1), wrap(y0 + 1));
    let s = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (ux, uy) = (s(fx), s(fy));
    let v00 = hash2(x0w, y0w);
    let v10 = hash2(x1w, y0w);
    let v01 = hash2(x0w, y1w);
    let v11 = hash2(x1w, y1w);
    let a = v00 + (v10 - v00) * ux;
    let b = v01 + (v11 - v01) * ux;
    a + (b - a) * uy
}

fn fbm(x: f32, y: f32, p0: i64, oct: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut p = p0;
    let mut norm = 0.0;
    for _ in 0..oct {
        sum += amp * vnoise(x, y, p);
        norm += amp;
        amp *= 0.5;
        p *= 2;
    }
    sum / norm
}

fn hsv2rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

fn confetti(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let wu = u + 0.12 * fbm(u, v, 6, 4);
    let wv = v + 0.12 * fbm(u + 5.2, v + 1.3, 6, 4);
    let base = fbm(wu, wv, 5, 5);
    let mid = fbm(wu + 2.7, wv + 8.1, 18, 4);
    let fine = fbm(wu + 9.3, wv + 3.4, 40, 3);
    let huef = (base * 1.0 + mid * 1.6 + fine * 2.2).fract();
    let bands = 14.0_f32;
    let hue_q = (huef * bands).floor() / bands + 0.5 / bands;
    let hue = hue_q * 360.0 + phase;
    let sat = (0.82 + 0.18 * fine).clamp(0.0, 1.0);
    let val = (0.55 + 0.42 * fbm(wu + 1.1, wv + 6.6, 24, 3)).clamp(0.0, 1.0);
    let rgb = hsv2rgb(hue, sat, val);
    pack_rgb(rgb)
}

fn pack_rgb(rgb: [f32; 3]) -> [u8; 3] {
    [
        (rgb[0] * 255.0).clamp(0.0, 255.0) as u8,
        (rgb[1] * 255.0).clamp(0.0, 255.0) as u8,
        (rgb[2] * 255.0).clamp(0.0, 255.0) as u8,
    ]
}

// ---------------------------------------------------------------------------
// LIQUID MARBLE. Iterated domain warping (flow noise): the field is sampled
// through two layers of fbm displacement, so straight contours bend into
// molten, marbled veins. Every fbm is the period-1 wrapped `vnoise`, so warping
// the coordinates keeps the whole field period-1 -> it still tiles and the UV
// scroll wraps seamlessly, the colour just appears to FLOW like a lava lamp.
// ---------------------------------------------------------------------------
fn liquid(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.5;
    // layer 1 warp vector
    let q0 = fbm(u, v, 4, 5);
    let q1 = fbm(u + 3.1, v + 6.2, 4, 5);
    // layer 2 warp vector, displaced by layer 1
    let r0 = fbm(u + warp * q0 + 1.7, v + warp * q1 + 9.2, 5, 5);
    let r1 = fbm(u + warp * q0 + 8.3, v + warp * q1 + 2.8, 5, 5);
    // final field, displaced by layer 2
    let f = fbm(u + warp * r0, v + warp * r1, 6, 5);
    // veins: a banded sine of the field carves thin filaments between colour pools
    let veins = ((f * 11.0 + r0 * 3.0) * TAU).sin() * 0.5 + 0.5;
    let hue = (f * 2.4 + r0 * 0.9 + phase).fract() * 360.0;
    let sat = (0.80 + 0.20 * q1).clamp(0.0, 1.0);
    let val = (0.30 + 0.68 * veins).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

// ---------------------------------------------------------------------------
// MOIRE. Two pairs of plane-wave gratings whose frequencies differ by one
// cycle beat against each other into a slow interference envelope. Integer
// frequencies make every grating period-1, so the field tiles exactly. The
// phase term + UV scroll slide the gratings, sweeping the moire fringes.
// ---------------------------------------------------------------------------
fn moire(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let g = |a: f32, b: f32, ph: f32| (TAU * (a * u + b * v) + ph).sin();
    let p = phase * TAU;
    // close-frequency pairs -> low-frequency beat in two orientations
    let m1 = g(9.0, 4.0, p) + g(10.0, 5.0, -p);
    let m2 = g(4.0, 9.0, p * 1.3) + g(5.0, 8.0, -p);
    let field = (m1 * m2) * 0.25 + 0.5; // 0..1 interference
    let hue = (field * 1.6 + 0.12 * m1 + phase).fract() * 360.0;
    let sat = 0.95;
    let val = (0.42 + 0.5 * ((field * 6.0).sin() * 0.5 + 0.5)).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

// ---------------------------------------------------------------------------
// KALEIDOSCOPE. A superposition of gratings on the square symmetry axes
// (horizontal, vertical, both diagonals) builds an 8-point mandala; a
// cos-based radial term (periodic, so still tiling) pulls the hue into rings
// around each tile centre. All wavevectors are integer -> exact tiling.
// ---------------------------------------------------------------------------
fn kaleido(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let g = |a: f32, b: f32| (TAU * (a * u + b * v)).cos();
    let k = 6.0;
    let mandala = (g(k, 0.0) + g(0.0, k) + 0.7 * g(k, k) + 0.7 * g(k, -k)) * 0.25; // ~ -1..1
                                                                                   // periodic radial: peaks at tile centre + corners, troughs at edge midpoints
    let radial = ((TAU * u).cos() + (TAU * v).cos()) * 0.5;
    let field = mandala * 0.5 + 0.5;
    let hue = (field + 0.35 * radial + phase).fract() * 360.0;
    let sat = (0.85 + 0.15 * radial).clamp(0.0, 1.0);
    let val = (0.45 + 0.5 * (mandala * 0.5 + 0.5)).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

// ---------------------------------------------------------------------------
// HOLOGRAPHIC FOIL. A smooth iridescent sweep with fine foil striations -- the
// flat base the holo material treatment sits on (Fresnel rim + crawling
// specular, applied as vmat patches in main). Tiling: integer-frequency hue
// sweep + foil lines, fbm-warped (period-1) so the runtime scroll still wraps.
// ---------------------------------------------------------------------------
fn holo(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.06 * fbm(u + 1.3, v + 4.1, 6, 3);
    let sweep = (u * 2.0 + v + warp).fract(); // smooth diagonal iridescence (tiles)
    let foil = ((u * 48.0 - v * 48.0) * TAU).sin() * 0.5 + 0.5; // fine foil lines (tiles)
    let hue = (sweep + 0.04 * foil + phase).fract() * 360.0;
    let sat = (0.45 + 0.30 * foil).clamp(0.0, 1.0); // desaturate on the foil highlights
    let val = (0.72 + 0.28 * foil).clamp(0.0, 1.0);
    pack_rgb(hsv2rgb(hue, sat, val))
}

// ---------------------------------------------------------------------------
// GLITCH / CRT. Rainbow base broken by horizontal scanlines, datamosh bands that
// slide sideways, and per-channel chromatic aberration (R/G/B sampled at slightly
// offset u) for the classic broken-signal fringing. Tiling: scanline + hue are
// integer-frequency; band displacement is hashed per row-block (period-1).
// ---------------------------------------------------------------------------
fn glitch(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let band = (v * 28.0).floor() as i64; // 28 horizontal bands per tile
    let r = hash2(band, 7);
    let shift = if r > 0.62 { (r - 0.62) * 0.9 } else { 0.0 }; // glitch bands slide
    let warp = 0.15 * fbm(u, v, 10, 3);
    let hue_at = |off: f32| (((u + shift + warp + off) * 2.0 + phase).fract()) * 360.0;
    // chromatic aberration: split the three channels across a small u offset
    let s = 0.92;
    let val = 0.9;
    let rr = hsv2rgb(hue_at(0.0), s, val)[0];
    let gg = hsv2rgb(hue_at(0.010), s, val)[1];
    let bb = hsv2rgb(hue_at(0.020), s, val)[2];
    // scanlines: fine dark horizontal lines (integer freq -> tiles)
    let scan = 0.55 + 0.45 * ((v * 512.0 * TAU).sin().abs());
    pack_rgb([rr * scan, gg * scan, bb * scan])
}

// FLIR "ironbow" thermal ramp: black -> indigo -> magenta -> red -> orange ->
// yellow -> white as t goes 0..1.
fn thermal_color(t: f32) -> [f32; 3] {
    const STOPS: &[(f32, [f32; 3])] = &[
        (0.00, [0.0, 0.0, 0.05]),
        (0.22, [0.20, 0.0, 0.45]),
        (0.42, [0.65, 0.0, 0.55]),
        (0.60, [0.95, 0.10, 0.10]),
        (0.76, [1.0, 0.55, 0.0]),
        (0.90, [1.0, 0.95, 0.25]),
        (1.00, [1.0, 1.0, 1.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if t <= b {
            let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
            return [
                ca[0] + (cb[0] - ca[0]) * k,
                ca[1] + (cb[1] - ca[1]) * k,
                ca[2] + (cb[2] - ca[2]) * k,
            ];
        }
    }
    [1.0, 1.0, 1.0]
}

// THERMAL / x-ray: a domain-warped fbm "heat" field mapped through the ironbow
// ramp -> looks like a heat-signature skin that flows with the UV scroll.
fn thermal(u: f32, v: f32, phase: f32) -> [u8; 3] {
    let warp = 0.25 * fbm(u + 1.7, v + 4.2, 6, 4);
    let heat =
        (fbm(u + warp, v + warp, 6, 5) * 1.5 + 0.25 * fbm(u + 9.0, v + 2.0, 24, 3) + phase).fract();
    pack_rgb(thermal_color(heat))
}

fn body_pixel(style: &str, u: f32, v: f32, phase: f32) -> [u8; 3] {
    match style {
        "liquid" | "hololiquid" => liquid(u, v, phase),
        "moire" => moire(u, v, phase),
        "kaleido" | "kaleidoscope" => kaleido(u, v, phase),
        "holo" => holo(u, v, phase),
        "glitch" | "crt" => glitch(u, v, phase),
        "thermal" | "xray" => thermal(u, v, phase),
        _ => confetti(u, v, phase),
    }
}

fn rgba8_mut(img: &mut Image) -> anyhow::Result<&mut Vec<u8>> {
    match &mut img.data {
        ImageData::Rgba8(v) => Ok(v),
        ImageData::Rgba16F(_) => anyhow::bail!("unexpected HDR texture"),
    }
}

fn paint_body(img: &mut Image, style: &str, phase: f32) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let c = body_pixel(style, u, v, phase);
            let i = ((y * w + x) * 4) as usize;
            px[i] = c[0];
            px[i + 1] = c[1];
            px[i + 2] = c[2];
        }
    }
    Ok(())
}

// Smooth flowing rainbow gradient (tiling: integer hue cycles in u,v). Used for the
// GUN: its mesh was built for a flat placeholder, so some parts have collapsed UVs
// that sample a single texel. With confetti that shows as a jarring solid block;
// with a smooth gradient the block matches its neighbors and disappears, and the
// scroll makes the whole gun flow through the spectrum.
fn paint_gradient(img: &mut Image, phase: f32) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            // 2 hue cycles across u + 1 across v -> seamless, smooth diagonal sweep.
            let hue = ((u * 2.0 + v).fract() + phase).fract() * 360.0;
            let rgb = hsv2rgb(hue, 0.95, 0.92);
            let i = ((y * w + x) * 4) as usize;
            px[i] = (rgb[0] * 255.0) as u8;
            px[i + 1] = (rgb[1] * 255.0) as u8;
            px[i + 2] = (rgb[2] * 255.0) as u8;
        }
    }
    Ok(())
}

// ---- in-place vmat double edits (NOT a re-encode; preserves material framing) ----

fn vparam_index(v: &Value, name: &str) -> Option<usize> {
    v.get("m_vectorParams")?
        .as_array()?
        .iter()
        .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
}

// Single scalar edit for a float param (m_floatParams[i].m_flValue). The probe
// confirmed these are real stored doubles, patchable via the same double patcher
// as the vector params (so a holo scale/exponent flips just those 8 bytes).
fn fparam_edit(v: &Value, name: &str, val: f64) -> Vec<(Vec<Seg>, f64)> {
    let Some(i) = v
        .get("m_floatParams")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .position(|p| p.get("m_name").and_then(Value::as_str) == Some(name))
        })
    else {
        return Vec::new();
    };
    vec![(
        vec![
            Seg::Key("m_floatParams".to_string()),
            Seg::Index(i),
            Seg::Key("m_flValue".to_string()),
        ],
        val,
    )]
}

// Edits for specific components of a vector param (e.g. RGB of a tint/color const).
fn vcomp_edits(v: &Value, name: &str, comps: &[(usize, f64)]) -> Vec<(Vec<Seg>, f64)> {
    let Some(i) = vparam_index(v, name) else {
        return Vec::new();
    };
    comps
        .iter()
        .map(|&(k, val)| {
            (
                vec![
                    Seg::Key("m_vectorParams".to_string()),
                    Seg::Index(i),
                    Seg::Key("m_value".to_string()),
                    Seg::Index(k),
                ],
                val,
            )
        })
        .collect()
}

fn scroll_xy_edits(v: &Value, name: &str, xy: [f64; 2]) -> Vec<(Vec<Seg>, f64)> {
    vcomp_edits(v, name, &[(0, xy[0]), (1, xy[1])])
}

// Apply required double edits in place (errors if any path isn't a real double).
fn patch_required(bytes: &[u8], edits: &[(Vec<Seg>, f64)], what: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(!edits.is_empty(), "no edits found for {what}");
    morphic::patch_kv3_resource_doubles(bytes, edits)
        .map_err(|e| anyhow::anyhow!("in-place patch for {what} failed: {e}"))
}

// Apply optional edits (e.g. scroll on a vector that may be a tagless zero); keep
// the input bytes if the patch can't land.
fn patch_optional(bytes: Vec<u8>, edits: &[(Vec<Seg>, f64)]) -> (Vec<u8>, bool) {
    if edits.is_empty() {
        return (bytes, false);
    }
    match morphic::patch_kv3_resource_doubles(&bytes, edits) {
        Ok(b) => (b, true),
        Err(_) => (bytes, false),
    }
}

fn main() -> anyhow::Result<()> {
    // Args: <pak01_dir.vpk> <out_dir.vpk|--png file> [--style confetti|liquid|moire|kaleido]
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut style = "confetti".to_string();
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == "--style" {
            style = raw.get(i + 1).cloned().expect("--style needs a value");
            i += 2;
        } else {
            pos.push(raw[i].clone());
            i += 1;
        }
    }
    let pak = pos.first().cloned().expect(
        "usage: reskin_chrono_trippy <pak01_dir.vpk> <out_dir.vpk|--png file> \
         [--style confetti|liquid|moire|kaleido]",
    );
    let arg2 = pos
        .get(1)
        .cloned()
        .expect("second arg: <out_dir.vpk> or --png <file>");
    eprintln!("style: {style}");

    let body_color_bytes = vpkmerge_core::read_vpk_entry(&pak, BODY_COLOR)?;

    // --- body albedo: tiling procedural rainbow (BC7 re-encode), chosen by --style ---
    let mut body = morphic::decode(&body_color_bytes)?;
    paint_body(&mut body, &style, 0.0)?;

    if arg2 == "--png" {
        let out = pos.get(2).cloned().expect("--png needs an output path");
        let png = morphic::encode_image(&body, TextureFormat::PngRgba8888)?;
        std::fs::write(&out, &png)?;
        println!("wrote preview PNG: {out} ({}x{})", body.width, body.height);
        return Ok(());
    }
    let out = arg2;

    let new_body_color = morphic::replace_mip_chain(&body_color_bytes, &body)?;
    eprintln!("body albedo re-encoded ({}x{})", body.width, body.height);

    // --- gun albedo: rainbow in a copy of the body's BC7 sRGB container, packed
    //     over the gun's flat 4x4 g_tColor placeholder path ---
    let mut gun = morphic::decode(&body_color_bytes)?;
    paint_gradient(&mut gun, 0.33)?; // smooth flowing rainbow (UV-robust, no block artifact)
    let new_gun_color = morphic::replace_mip_chain(&body_color_bytes, &gun)?;
    eprintln!("gun albedo re-encoded ({}x{})", gun.width, gun.height);

    // --- body vmat: flow the albedo + self-illum (in place) ---
    let body_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, BODY_VMAT)?;
    let bv = morphic::decode_kv3_resource(&body_vmat_bytes)?;
    let mut body_edits = scroll_xy_edits(&bv, "g_vAlbedoScrollSpeed1", [SCROLL[0], SCROLL[1]]);
    body_edits.extend(scroll_xy_edits(
        &bv,
        "g_vSelfIllumScrollSpeed1",
        [SCROLL[0], SCROLL[1]],
    ));
    if style == "holo" || style == "hololiquid" {
        // Crawling specular: scroll the normal/roughness map FASTER than the
        // albedo so the moving shine separates from the colour -> a view + time
        // dependent foil sheen, and a faux-parallax depth shimmer (the lit relief
        // and the colour travel at different rates).
        body_edits.extend(scroll_xy_edits(
            &bv,
            "g_vNormalAndRoughnessScrollSpeed1",
            [0.12, 0.07],
        ));
        // Widen + brighten the Fresnel-masked self-illum so the edges glow as you
        // orbit the hero (the real holographic, view-dependent cue). Rim tint kept
        // white so it reads the iridescent albedo colour; albedo factor left at 1.
        body_edits.extend(fparam_edit(&bv, "g_flSelfIllumScale1", 5.5));
        body_edits.extend(fparam_edit(&bv, "g_flSelfIllumFresnelMaskExponent", 2.5));
        body_edits.extend(vcomp_edits(
            &bv,
            "g_vSelfIllumFresnelMaskTint1",
            &[(0, 1.0), (1, 1.0), (2, 1.0)],
        ));
        eprintln!("holo: Fresnel rim + crawling specular patches added");
    }
    let new_body_vmat = patch_required(&body_vmat_bytes, &body_edits, "body scroll")?;
    eprintln!("body vmat scroll set ({} edits)", body_edits.len());

    // --- gun vmat: null the tan TextureColor1 fallback to white so the rainbow
    //     shows pure, then flow it (scroll best-effort) ---
    let gun_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, GUN_VMAT)?;
    let gv = morphic::decode_kv3_resource(&gun_vmat_bytes)?;
    let gun_white = vcomp_edits(&gv, "TextureColor1", &[(0, 1.0), (1, 1.0), (2, 1.0)]);
    let gun_step = patch_required(&gun_vmat_bytes, &gun_white, "gun TextureColor1->white")?;
    let (new_gun_vmat, gun_scrolled) = patch_optional(
        gun_step,
        &scroll_xy_edits(&gv, "g_vAlbedoScrollSpeed1", [SCROLL[0], SCROLL[1]]),
    );
    eprintln!("gun vmat: tint nulled, scroll={gun_scrolled}");

    // --- head glass: brighten the near-black dome tint so it reads CLEAR/light,
    //     revealing the glowing hourglass behind it ---
    let head_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, HEADGLASS_VMAT)?;
    let hv = morphic::decode_kv3_resource(&head_vmat_bytes)?;
    // 0.043 (near-black smoke) -> a light cool tint = clear glass.
    let head_clear = vcomp_edits(&hv, "TextureColor1", &[(0, 0.80), (1, 0.86), (2, 0.96)]);
    let new_head_vmat = patch_required(&head_vmat_bytes, &head_clear, "head glass clear")?;
    eprintln!("head glass brightened to clear");

    // --- hourglass: retint its self-illum glow crimson -> neon cyan ---
    let hg_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, HOURGLASS_VMAT)?;
    let hgv = morphic::decode_kv3_resource(&hg_vmat_bytes)?;
    let hg_neon = vcomp_edits(&hgv, "g_vSelfIllumTint1", &[(0, 0.10), (1, 0.95), (2, 1.0)]);
    let new_hg_vmat = patch_required(&hg_vmat_bytes, &hg_neon, "hourglass neon glow")?;
    eprintln!("hourglass glow retinted neon cyan");

    let readme = format!(
        "Paradox Trippy (animated) skin -- body + gun + head\n\
        ===================================================\n\
        vpkmerge test build. Hero: Paradox (chrono). Style: {style}. All vmat edits are\n\
        byte-faithful in-place double patches (no KV3 re-encode).\n\
        - body albedo: tiling {style} RAINBOW; gun albedo: smooth flowing gradient\n\
          (gun UVs are collapsed, so a detailed pattern would block up there).\n\
        - g_vAlbedoScrollSpeed1 (+ body g_vSelfIllumScrollSpeed1) set so it FLOWS at runtime.\n\
        - head glass: dome tint brightened (near-black -> light) to read CLEAR.\n\
        - hourglass: self-illum glow retinted crimson -> neon cyan.\n\
        - normal + AO maps untouched: surface form stays put, only color sweeps.\n"
    );

    vpkmerge_core::pack(
        &[
            (BODY_COLOR, new_body_color.as_slice()),
            (BODY_VMAT, new_body_vmat.as_slice()),
            (GUN_COLOR, new_gun_color.as_slice()),
            (GUN_VMAT, new_gun_vmat.as_slice()),
            (HEADGLASS_VMAT, new_head_vmat.as_slice()),
            (HOURGLASS_VMAT, new_hg_vmat.as_slice()),
            ("README.txt", readme.as_bytes()),
        ],
        &out,
    )?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
