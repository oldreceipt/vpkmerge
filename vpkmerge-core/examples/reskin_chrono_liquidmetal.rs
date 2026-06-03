// Paradox "liquid metal" skin: animate the SURFACE, not the colour.
//
// New mechanism vs the trippy/prism work (which all scroll *albedo*): here the
// albedo is a near-static metal tone and the motion comes from a flowing-ripple
// NORMAL map that scrolls (g_vNormalAndRoughnessScrollSpeed1), so the reflected
// environment crawls across the body like mercury / molten gold. Chrome shine is
// a uniform LOW roughness (the Vindicta-dress lesson).
//
// chrono's g_tNormalRoughness is a flat 4x4 placeholder (128,128,245,255 =
// flat normal + 0.96 roughness), so we override it full-res exactly like the
// trippy gun overrode its 4x4 g_tColor placeholder: paint into a copy of the
// body's 4096 BC7 container and pack at the placeholder's entry path. The
// material already points there, so no .vmat repoint.
//
// Layered so the colorspace guess can't sink the effect:
//   - shine   <- TextureRoughness1 -> low   (material scalar, unambiguous)
//   - colour  <- albedo override            (sRGB colour, unambiguous)
//   - flow    <- ripple normal + scroll      (animates regardless of gamma)
// The g_tNormalRoughness slot is read LINEAR by the shader, so we store linear
// values directly; --gamma-precomp flips to inverse-sRGB if an in-game test
// shows a constant surface tilt (would mean the slot honored an sRGB flag).
//
// All vmat edits are byte-faithful IN-PLACE double patches (a full
// encode_kv3_resource re-serialize renders as the engine error shader in-game).
//
// usage:
//   cargo run --release --example reskin_chrono_liquidmetal -- <pak01_dir.vpk> <out_dir.vpk> [--flavor chrome|mercury|gold|molten] [--gamma-precomp]
//   cargo run --release --example reskin_chrono_liquidmetal -- <pak01_dir.vpk> --png <prefix> [--flavor ...]
use morphic::kv3::{Seg, Value};
use morphic::{Image, ImageData, TextureFormat};
use std::f32::consts::TAU;

const BODY_COLOR: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_color_png_d1d22ba7.vtex_c";
const BODY_NORMAL: &str =
    "models/heroes_staging/chrono/materials/chrono_v2_vmat_g_tnormalroughness_ce38f34.vtex_c";
const BODY_VMAT: &str = "models/heroes_staging/chrono/materials/chrono_v2.vmat_c";
const GUN_COLOR: &str =
    "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun_vmat_g_tcolor_7d4419c1.vtex_c";
const GUN_NORMAL: &str =
    "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun_vmat_g_tnormalroughness_7cd9ceac.vtex_c";
const GUN_VMAT: &str = "models/heroes_staging/chrono/chrono_gun/materials/chrono_gun.vmat_c";

// UV units/sec for the normal scroll = how fast the metal flows. Slow + diagonal
// reads as a slow molten crawl. The chrono body UVs are fragmented/mirrored, so
// the flow runs different directions per island = a turbulent, mercury-like churn.
const FLOW: [f64; 4] = [0.04, 0.03, 0.0, 0.0];
// Chrome/mirror shine. Low + uniform. 0 = perfect mirror; ~0.12 keeps a touch of
// softness so highlights have body. The original chrono body is 0.96 (matte).
const ROUGHNESS_LOW: f64 = 0.12;

// ---------------------------------------------------------------------------
// Period-1 value noise (tiles seamlessly so the scroll wraps). Same family as
// the trippy generators.
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

// ---------------------------------------------------------------------------
// Flowing-liquid height field, period-1 in u and v. Domain-warped low-frequency
// value noise (organic molten blobs) plus two integer-frequency long ripples for
// a directional current. Smooth on purpose: clean liquid waves, not noise grit.
// ---------------------------------------------------------------------------
fn height(u: f32, v: f32) -> f32 {
    // low-frequency + few octaves -> smooth, large molten waves (so the slope
    // stays gentle and the normal map reads pastel, not clipped).
    let wx = fbm(u, v, 2, 3);
    let wy = fbm(u + 3.3, v + 1.7, 2, 3);
    let warp = 0.45;
    let blob = fbm(u + warp * wx, v + warp * wy, 3, 3);
    let ripple = 0.26 * (TAU * (2.0 * u + v)).sin() + 0.18 * (TAU * (u + 2.0 * v + 0.3)).sin();
    blob * 0.78 + (ripple * 0.5 + 0.5) * 0.22
}

// Tangent-space normal from the height field (finite differences over a few
// texels for a smooth slope), returned as unit (nx, ny, nz) with nz > 0.
fn surface_normal(u: f32, v: f32, eps: f32, bump: f32) -> [f32; 3] {
    let hl = height(u - eps, v);
    let hr = height(u + eps, v);
    let hd = height(u, v - eps);
    let hu = height(u, v + eps);
    let nx = -(hr - hl) / (2.0 * eps) * bump;
    let ny = -(hu - hd) / (2.0 * eps) * bump;
    let nz = 1.0;
    let inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
    [nx * inv, ny * inv, nz * inv]
}

// ---------------------------------------------------------------------------
// Colour ramps. v01 is the (0..1) liquid value field; brightness rides the
// height so the metal reads as pooled/flowing rather than flat paint.
// ---------------------------------------------------------------------------
fn metal_rgb(flavor: &str, v01: f32) -> [f32; 3] {
    let t = v01.clamp(0.0, 1.0);
    match flavor {
        "mercury" => {
            let g = 0.34 + 0.62 * t;
            [g * 0.95, g * 0.98, (g * 1.06).min(1.0)]
        }
        "gold" => {
            // dark gold pools -> bright molten-gold veins
            let base = [1.0, 0.80, 0.40];
            let vein = [1.0, 0.95, 0.70];
            let g = 0.42 + 0.55 * t;
            let k = (t.powf(3.0)).clamp(0.0, 1.0); // veins only at the top end
            [
                (g * base[0]) * (1.0 - k) + vein[0] * k,
                (g * base[1]) * (1.0 - k) + vein[1] * k,
                (g * base[2]) * (1.0 - k) + vein[2] * k,
            ]
        }
        "molten" => molten_ramp(t),
        // chrome (default): neutral, faintly cool, bright caustic veins
        _ => {
            let g = 0.45 + 0.5 * t;
            let k = (t.powf(4.0)).clamp(0.0, 1.0);
            let base = [g * 0.97, g * 0.99, (g * 1.03).min(1.0)];
            [
                base[0] * (1.0 - k) + k,
                base[1] * (1.0 - k) + k,
                base[2] * (1.0 - k) + k,
            ]
        }
    }
}

// lava-metal: dark bronze -> orange -> hot yellow-white
fn molten_ramp(t: f32) -> [f32; 3] {
    const STOPS: &[(f32, [f32; 3])] = &[
        (0.00, [0.18, 0.07, 0.03]),
        (0.45, [0.55, 0.18, 0.05]),
        (0.72, [0.95, 0.45, 0.08]),
        (0.90, [1.0, 0.80, 0.30]),
        (1.00, [1.0, 0.97, 0.78]),
    ];
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
    [1.0, 0.97, 0.78]
}

fn srgb_encode(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn rgba8_mut(img: &mut Image) -> anyhow::Result<&mut Vec<u8>> {
    match &mut img.data {
        ImageData::Rgba8(v) => Ok(v),
        ImageData::Rgba16F(_) => anyhow::bail!("unexpected HDR texture"),
    }
}

// Paint the packed normal-roughness: R,G = tangent normal.xy remapped to [0,1],
// B = roughness, A = 255 (matches placeholder; nz is reconstructed in-shader).
// gamma_precomp stores inverse-sRGB so an sRGB-sampled slot recovers the linear
// values; default stores linear directly (the slot reads linear).
fn paint_normal_roughness(img: &mut Image, gamma_precomp: bool) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let eps = 2.0 / w as f32;
    // small bump: the finite-diff derivative of the height field is already
    // large, so a fraction keeps tangent slopes gentle (pastel normal, ~<35deg).
    let bump = 0.05;
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let n = surface_normal(u, v, eps, bump);
            let nrx = n[0] * 0.5 + 0.5;
            let nry = n[1] * 0.5 + 0.5;
            // rougher on the steep ripple flanks so the flow reads even in flat light
            let slope = (1.0 - n[2]).clamp(0.0, 1.0);
            let rough = (ROUGHNESS_LOW as f32 + 0.16 * slope).clamp(0.0, 0.6);
            let (rr, gg, bb) = if gamma_precomp {
                (srgb_encode(nrx), srgb_encode(nry), srgb_encode(rough))
            } else {
                (nrx, nry, rough)
            };
            let i = ((y * w + x) * 4) as usize;
            px[i] = byte(rr);
            px[i + 1] = byte(gg);
            px[i + 2] = byte(bb);
            px[i + 3] = 255;
        }
    }
    Ok(())
}

// Paint the metal albedo. Brightness rides the same height field as the normal,
// so bright "caustic" veins sit on the wave crests -> they appear to flow with
// the scroll. Colour is sRGB (stored directly, like the trippy albedo).
fn paint_albedo(img: &mut Image, flavor: &str) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let v01 = height(u, v);
            let rgb = metal_rgb(flavor, v01);
            let i = ((y * w + x) * 4) as usize;
            px[i] = byte(rgb[0]);
            px[i + 1] = byte(rgb[1]);
            px[i + 2] = byte(rgb[2]);
        }
    }
    Ok(())
}

// Smooth flowing metal for the GUN: its mesh was built for a flat placeholder, so
// some parts have collapsed UVs that sample a single texel. A smooth diagonal
// value gradient (vs a detailed field) keeps those patches matching their
// neighbors instead of blocking up.
fn paint_gun_albedo(img: &mut Image, flavor: &str) -> anyhow::Result<()> {
    let (w, h) = (img.width, img.height);
    let px = rgba8_mut(img)?;
    for y in 0..h {
        let v = y as f32 / h as f32;
        for x in 0..w {
            let u = x as f32 / w as f32;
            let t = ((u * 1.5 + v).fract() * TAU).sin() * 0.5 + 0.5;
            let rgb = metal_rgb(flavor, t);
            let i = ((y * w + x) * 4) as usize;
            px[i] = byte(rgb[0]);
            px[i + 1] = byte(rgb[1]);
            px[i + 2] = byte(rgb[2]);
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

fn rough_low_edits(v: &Value) -> Vec<(Vec<Seg>, f64)> {
    vcomp_edits(
        v,
        "TextureRoughness1",
        &[(0, ROUGHNESS_LOW), (1, ROUGHNESS_LOW), (2, ROUGHNESS_LOW)],
    )
}

fn patch_required(bytes: &[u8], edits: &[(Vec<Seg>, f64)], what: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(!edits.is_empty(), "no edits found for {what}");
    morphic::patch_kv3_resource_doubles(bytes, edits)
        .map_err(|e| anyhow::anyhow!("in-place patch for {what} failed: {e}"))
}

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
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut flavor = "chrome".to_string();
    let mut gamma_precomp = false;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--flavor" => {
                flavor = raw.get(i + 1).cloned().expect("--flavor needs a value");
                i += 2;
            }
            "--gamma-precomp" => {
                gamma_precomp = true;
                i += 1;
            }
            _ => {
                pos.push(raw[i].clone());
                i += 1;
            }
        }
    }
    let pak = pos.first().cloned().expect(
        "usage: reskin_chrono_liquidmetal <pak01_dir.vpk> <out_dir.vpk|--png prefix> \
         [--flavor chrome|mercury|gold|molten] [--gamma-precomp]",
    );
    let arg2 = pos
        .get(1)
        .cloned()
        .expect("second arg: <out_dir.vpk> or --png <prefix>");
    eprintln!("flavor: {flavor}  gamma_precomp: {gamma_precomp}");

    // Preview mode renders the resolution-independent fields at a small size so
    // they are quick to inspect (the bake below paints into the full 4096 container).
    if arg2 == "--png" {
        let prefix = pos.get(2).cloned().expect("--png needs an output prefix");
        let blank = || Image {
            width: 768,
            height: 768,
            data: ImageData::Rgba8(vec![255u8; 768 * 768 * 4]),
        };
        let mut pa = blank();
        paint_albedo(&mut pa, &flavor)?;
        let mut pn = blank();
        paint_normal_roughness(&mut pn, gamma_precomp)?;
        for (img, suffix) in [(&pa, "albedo"), (&pn, "normalroughness")] {
            let png = morphic::encode_image(img, TextureFormat::PngRgba8888)?;
            let path = format!("{prefix}_{suffix}.png");
            std::fs::write(&path, &png)?;
            println!("wrote {path} ({}x{})", img.width, img.height);
        }
        return Ok(());
    }
    let out = arg2;

    // The body's 4096 BC7 container is the template for every override (both the
    // metal albedo and the full-res normal-roughness that replaces the 4x4 slot).
    let body_color_bytes = vpkmerge_core::read_vpk_entry(&pak, BODY_COLOR)?;

    let mut body_albedo = morphic::decode(&body_color_bytes)?;
    paint_albedo(&mut body_albedo, &flavor)?;

    let mut body_nr = morphic::decode(&body_color_bytes)?;
    paint_normal_roughness(&mut body_nr, gamma_precomp)?;

    let new_body_color = morphic::replace_mip_chain(&body_color_bytes, &body_albedo)?;
    let new_body_nr = morphic::replace_mip_chain(&body_color_bytes, &body_nr)?;
    eprintln!("body albedo + normal-roughness re-encoded");

    // gun: smooth metal albedo + ripple normal-roughness in the same container
    let mut gun_albedo = morphic::decode(&body_color_bytes)?;
    paint_gun_albedo(&mut gun_albedo, &flavor)?;
    let new_gun_color = morphic::replace_mip_chain(&body_color_bytes, &gun_albedo)?;
    let mut gun_nr = morphic::decode(&body_color_bytes)?;
    paint_normal_roughness(&mut gun_nr, gamma_precomp)?;
    let new_gun_nr = morphic::replace_mip_chain(&body_color_bytes, &gun_nr)?;
    eprintln!("gun albedo + normal-roughness re-encoded");

    // --- body vmat: flow the normal, drop roughness (no albedo scroll for chrome;
    //     slow albedo drift for the warm flavors so the molten colour creeps too) ---
    let body_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, BODY_VMAT)?;
    let bv = morphic::decode_kv3_resource(&body_vmat_bytes)?;
    let mut body_edits =
        scroll_xy_edits(&bv, "g_vNormalAndRoughnessScrollSpeed1", [FLOW[0], FLOW[1]]);
    body_edits.extend(rough_low_edits(&bv));
    if flavor == "gold" || flavor == "molten" {
        body_edits.extend(scroll_xy_edits(
            &bv,
            "g_vAlbedoScrollSpeed1",
            [FLOW[0] * 0.5, FLOW[1] * 0.5],
        ));
    }
    let new_body_vmat = patch_required(&body_vmat_bytes, &body_edits, "body liquid-metal")?;
    eprintln!(
        "body vmat: flow + low roughness ({} edits)",
        body_edits.len()
    );

    // --- gun vmat: null tan TextureColor1 -> white, drop roughness, flow normal ---
    let gun_vmat_bytes = vpkmerge_core::read_vpk_entry(&pak, GUN_VMAT)?;
    let gv = morphic::decode_kv3_resource(&gun_vmat_bytes)?;
    let mut gun_edits = vcomp_edits(&gv, "TextureColor1", &[(0, 1.0), (1, 1.0), (2, 1.0)]);
    gun_edits.extend(rough_low_edits(&gv));
    let gun_step = patch_required(&gun_vmat_bytes, &gun_edits, "gun tint+roughness")?;
    let (new_gun_vmat, gun_flowed) = patch_optional(
        gun_step,
        &scroll_xy_edits(&gv, "g_vNormalAndRoughnessScrollSpeed1", [FLOW[0], FLOW[1]]),
    );
    eprintln!("gun vmat: tint nulled + low roughness, flow={gun_flowed}");

    let readme = format!(
        "Paradox Liquid Metal skin -- body + gun\n\
        =======================================\n\
        vpkmerge test build. Hero: Paradox (chrono). Flavor: {flavor}.\n\
        New mechanism: the SURFACE flows, not the colour. A flowing-ripple normal\n\
        map (override of the flat 4x4 g_tNormalRoughness placeholder) is scrolled via\n\
        g_vNormalAndRoughnessScrollSpeed1, so the reflected environment crawls like\n\
        liquid metal. Shine = uniform low roughness (TextureRoughness1 -> {ROUGHNESS_LOW}).\n\
        Albedo is a near-static metal tone. All vmat edits are byte-faithful in-place\n\
        double patches (no KV3 re-encode). gamma_precomp={gamma_precomp}.\n"
    );

    vpkmerge_core::pack(
        &[
            (BODY_COLOR, new_body_color.as_slice()),
            (BODY_NORMAL, new_body_nr.as_slice()),
            (BODY_VMAT, new_body_vmat.as_slice()),
            (GUN_COLOR, new_gun_color.as_slice()),
            (GUN_NORMAL, new_gun_nr.as_slice()),
            (GUN_VMAT, new_gun_vmat.as_slice()),
            ("README.txt", readme.as_bytes()),
        ],
        &out,
    )?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
