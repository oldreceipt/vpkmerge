// Crank the DENSITY + chaos of Paradox's time-bomb volume particles, so the dome
// fills with more swirling (rainbow, prism-tinted) sprites -- the achievable
// "fill the volume with fractals" (true raymarched volumetrics need a shader).
//
// The aoe_* sprites emit via C_INIT_CreateWithinSphereTransform (throughout the
// dome volume). We multiply, IN PLACE (no re-encode -- that breaks particles):
//   m_nMaxParticles / nParticlesToEmit / flEmitRate  -> more sprites (fuller)
//   fSpeedMax                                          -> spread to fill more volume
//   flNoiseScaleLoc                                    -> chaotic/turbulent swirl
// (their textures are SHARED, so we can't fractal-texture the sprites; the fractal
// look lives in the gear/ground + the prism rainbow tint.)
//
// usage: cargo run --release --example crank_bomb_density -- <pak01_dir.vpk> <out_dir.vpk>
use morphic::kv3::{Seg, Value};

const VOLUME_PARTICLES: &[&str] = &[
    "particles/abilities/chrono/chrono_time_bomb_aoe_debris.vpcf_c",
    "particles/abilities/chrono/chrono_time_bomb_aoe_energy.vpcf_c",
    "particles/abilities/chrono/chrono_time_bomb_aoe_elec.vpcf_c",
    "particles/abilities/chrono/chrono_time_bomb_aoe_detail_proj.vpcf_c",
];

// field name -> (multiplier, is_plain_int). Plain ints (m_nMaxParticles) are patched
// directly; the rest are PF_TYPE_LITERAL input wrappers whose value lives in an inner
// `m_flLiteralValue` double (we only crank it when the input really is a literal, not
// a control-point-driven value).
fn target(k: &str) -> Option<(f64, bool)> {
    match k {
        "m_nMaxParticles" => Some((4.0, true)), // raise the system cap (plain int)
        "m_nParticlesToEmit" => Some((3.0, false)), // bigger burst
        "m_flEmitRate" => Some((2.5, false)),   // faster continuous emission
        "m_flEmissionDuration" => Some((1.4, false)),
        "m_fSpeedMax" => Some((1.35, false)), // fly out further -> fill more volume
        "m_flNoiseScaleLoc" => Some((2.0, false)), // more chaotic/turbulent swirl
        _ => None,
    }
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Double(d) => Some(*d),
        _ => None,
    }
}

// Recursively collect (path, new_value, is_int, label, old) for every target field,
// descending PF_TYPE_LITERAL wrappers to their `m_flLiteralValue`.
fn collect(v: &Value, path: &mut Vec<Seg>, out: &mut Vec<(Vec<Seg>, f64, bool, String, f64)>) {
    match v {
        Value::Object(o) => {
            for (k, val) in o {
                path.push(Seg::Key(k.clone()));
                if let Some((mult, plain_int)) = target(k) {
                    if plain_int {
                        if let Some(c) = as_num(val) {
                            out.push((path.clone(), c * mult, true, k.clone(), c));
                        }
                    } else if val.get("m_nType").and_then(Value::as_str) == Some("PF_TYPE_LITERAL")
                    {
                        if let Some(Value::Double(lit)) = val.get("m_flLiteralValue") {
                            path.push(Seg::Key("m_flLiteralValue".to_string()));
                            out.push((
                                path.clone(),
                                lit * mult,
                                false,
                                format!("{k}.literal"),
                                *lit,
                            ));
                            path.pop();
                        }
                    }
                }
                collect(val, path, out);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                path.push(Seg::Index(i));
                collect(val, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

// Apply one edit in place, trying int / f32 / f64 lanes; None if unpatchable.
fn try_set(bytes: &[u8], path: &[Seg], val: f64, is_int: bool) -> Option<Vec<u8>> {
    if is_int {
        return morphic::patch_kv3_resource_scalars(bytes, &[(path.to_vec(), val.round() as i64)])
            .ok();
    }
    if let Ok(b) = morphic::patch_kv3_resource_floats(bytes, &[(path.to_vec(), val as f32)]) {
        return Some(b);
    }
    morphic::patch_kv3_resource_doubles(bytes, &[(path.to_vec(), val)]).ok()
}

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a
        .next()
        .expect("usage: crank_bomb_density <pak01_dir.vpk> <out_dir.vpk>");
    let out = a.next().expect("out_dir.vpk");

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in VOLUME_PARTICLES {
        let bytes = vpkmerge_core::read_vpk_entry(&pak, entry)?;
        let value = morphic::decode_kv3_resource(&bytes)?;
        let mut edits = Vec::new();
        collect(&value, &mut Vec::new(), &mut edits);
        if edits.is_empty() {
            eprintln!(
                "  {}: no crankable fields found (skipped)",
                entry.rsplit('/').next().unwrap()
            );
            continue;
        }
        let mut cur = bytes.clone();
        let (mut applied, mut skipped) = (0, 0);
        for (path, val, is_int, name, old) in &edits {
            match try_set(&cur, path, *val, *is_int) {
                Some(b) => {
                    cur = b;
                    applied += 1;
                    eprintln!("    {name}: {old:.3} -> {val:.3}");
                }
                None => skipped += 1,
            }
        }
        eprintln!(
            "  {}: {applied} cranked, {skipped} unpatchable",
            entry.rsplit('/').next().unwrap()
        );
        if applied > 0 {
            packed.push((entry.to_string(), cur));
        }
    }

    anyhow::ensure!(!packed.is_empty(), "no volume particles cranked");
    let readme = "Paradox time-bomb DENSITY/CHAOS crank\n\
        =====================================\n\
        vpkmerge test build. In-place scalar boosts to the bomb's volume particles\n\
        (aoe debris/energy/elec/detail): more particles (maxparticles/emit count/rate),\n\
        wider speed spread, and amplified velocity noise so the dome fills with more\n\
        chaotically-swirling rainbow sprites. No re-encode (would break particles).\n";
    let mut refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    refs.push(("README.txt", readme.as_bytes()));
    vpkmerge_core::pack(&refs, &out)?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
