// "Time echo" ability VFX for Paradox (chrono): her projectiles, beams, tracers
// and trails leave LONGER, DENSER ribbon afterimages -- a time hero ghosting her
// own motion. All edits are IN PLACE (no re-encode -- that breaks particles),
// the same machinery as crank_bomb_density:
//   m_flConstantLifespan / m_f(l)Lifetime{Min,Max}  -> particles linger (the echo)
//   m_nMaxParticles                                  -> raise the cap so a trail can
//                                                       hold more segments at once
//   m_flEmitRate / m_nParticlesToEmit                -> denser ribbon / burst
// Scalars live either bare or wrapped in a PF_TYPE_LITERAL input; we crank either.
// Targets are discovered dynamically: every chrono ability .vpcf_c whose name
// reads as motion (trail/beam/tracer/streak/projectile). Pack -> one addon VPK.
//
// usage: cargo run --release --example time_echo -- <pak01_dir.vpk> <out_dir.vpk>
use morphic::kv3::{Seg, Value};

const MOTION_KEYWORDS: &[&str] = &["trail", "beam", "tracer", "streak", "projectile"];

// field name -> multiplier (DENSITY: how many particles coexist). (int vs float
// lane is chosen per-value in try_set.) Lifespan is NOT a named field here -- it's
// set via m_nOutputField (see OUTPUT_FIELD_MULT), so the linger comes from there.
fn target(k: &str) -> Option<f64> {
    match k {
        "m_nMaxParticles" => Some(3.5), // raise the cap so the longer-lived, denser stream isn't clipped
        "m_nParticlesToEmit" => Some(1.5),
        "m_flEmitRate" => Some(1.8),
        // named lifespan fields if a system happens to use them (most don't):
        "m_flConstantLifespan" => Some(1.7),
        "m_fLifetimeMin" | "m_flLifetimeMin" => Some(1.7),
        "m_fLifetimeMax" | "m_flLifetimeMax" => Some(1.7),
        _ => None,
    }
}

// m_nOutputField id -> multiplier. The Source 2 ParticleField ids (confirmed
// against the chrono data: field 1 carried rand(0.5..1) = seconds, field 7 was
// alpha 0.75..1, field 4 was roll 0..360): 1 = LifeDuration (the echo linger),
// 10 = TrailLength (ribbon length). We crank the op's m_InputValue for these.
fn output_field_mult(id: i64) -> Option<f64> {
    match id {
        1 => Some(1.7),  // LifeDuration -> particles linger = the time echo
        10 => Some(2.0), // TrailLength  -> longer ribbon
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

fn is_int_value(v: &Value) -> bool {
    matches!(v, Value::Int(_) | Value::UInt(_))
}

// (path, new_value, is_int, label, old). Handles a bare number OR a PF_TYPE_LITERAL
// wrapper (crank its inner m_flLiteralValue). Also reports any /life/i key we do
// NOT crank, so an unrecognized lifespan field name surfaces instead of silently
// being missed.
fn collect(
    v: &Value,
    path: &mut Vec<Seg>,
    out: &mut Vec<(Vec<Seg>, f64, bool, String, f64)>,
    seen_life: &mut std::collections::BTreeSet<String>,
) {
    match v {
        Value::Object(o) => {
            // Lifespan / trail-length pass: this op writes a particle field via
            // m_nOutputField; if it's LifeDuration (1) or TrailLength (10), crank
            // the literal/random of its m_InputValue (scoped to THIS op, so we
            // never touch the other fields).
            if let Some(id) = o
                .iter()
                .find(|(k, _)| k == "m_nOutputField")
                .and_then(|(_, x)| x.as_int())
            {
                if let Some(m) = output_field_mult(id) {
                    if let Some((_, inp)) = o.iter().find(|(k, _)| k == "m_InputValue") {
                        for key in ["m_flLiteralValue", "m_flRandomMin", "m_flRandomMax"] {
                            if let Some(c) = inp.get(key).and_then(as_num) {
                                let mut p = path.clone();
                                p.push(Seg::Key("m_InputValue".to_string()));
                                p.push(Seg::Key(key.to_string()));
                                out.push((p, c * m, false, format!("field{id}.{key}"), c));
                            }
                        }
                    }
                }
            }
            for (k, val) in o {
                if k.to_lowercase().contains("life") && target(k).is_none() {
                    seen_life.insert(k.clone());
                }
                path.push(Seg::Key(k.clone()));
                if let Some(mult) = target(k) {
                    if let Some(c) = as_num(val) {
                        out.push((path.clone(), c * mult, is_int_value(val), k.clone(), c));
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
                collect(val, path, out, seen_life);
                path.pop();
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                path.push(Seg::Index(i));
                collect(val, path, out, seen_life);
                path.pop();
            }
        }
        _ => {}
    }
}

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
        .expect("usage: time_echo <pak01_dir.vpk> <out_dir.vpk>");
    let out = a.next().expect("out_dir.vpk");

    let entries: Vec<String> = vpkmerge_core::inspect(&pak)?
        .file_paths
        .into_iter()
        .filter(|p| {
            p.starts_with("particles/abilities/chrono/")
                && p.ends_with(".vpcf_c")
                && MOTION_KEYWORDS.iter().any(|kw| p.contains(kw))
        })
        .collect();
    eprintln!("{} chrono motion particle systems matched", entries.len());

    let mut seen_life = std::collections::BTreeSet::new();
    let mut by_name: std::collections::BTreeMap<String, (usize, f64, f64)> =
        std::collections::BTreeMap::new();
    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let (mut total_applied, mut total_skipped) = (0usize, 0usize);

    for entry in &entries {
        let bytes = vpkmerge_core::read_vpk_entry(&pak, entry)?;
        let value = morphic::decode_kv3_resource(&bytes)?;
        let mut edits = Vec::new();
        collect(&value, &mut Vec::new(), &mut edits, &mut seen_life);
        if edits.is_empty() {
            continue;
        }
        let mut cur = bytes.clone();
        let (mut applied, mut skipped) = (0, 0);
        for (path, val, is_int, name, old) in &edits {
            match try_set(&cur, path, *val, *is_int) {
                Some(b) => {
                    cur = b;
                    applied += 1;
                    let e = by_name.entry(name.clone()).or_insert((0usize, *old, *val));
                    e.0 += 1;
                }
                None => skipped += 1,
            }
        }
        total_applied += applied;
        total_skipped += skipped;
        if applied > 0 {
            packed.push((entry.clone(), cur));
        }
    }

    eprintln!(
        "cranked {total_applied} fields across {} systems ({total_skipped} unpatchable)",
        packed.len()
    );
    for (name, (count, old, new)) in &by_name {
        eprintln!("  {name}: {count}x (e.g. {old:.3} -> {new:.3})");
    }
    if !seen_life.is_empty() {
        eprintln!("life-ish fields seen but NOT cranked: {seen_life:?}");
    }
    anyhow::ensure!(!packed.is_empty(), "nothing cranked");

    let readme = "Paradox TIME ECHO ability VFX\n\
        =============================\n\
        vpkmerge test build. In-place scalar boosts to chrono's motion particles\n\
        (projectiles/beams/tracers/streaks/trails): longer particle lifespan + higher\n\
        max-particle caps + denser emission, so each ability leaves longer, denser\n\
        ribbon afterimages -- a time hero ghosting her own motion. No re-encode\n\
        (would break particles). Merge with the prism for rainbow echoes.\n";
    let mut refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    refs.push(("README.txt", readme.as_bytes()));
    vpkmerge_core::pack(&refs, &out)?;
    println!("wrote addon VPK: {out}");
    Ok(())
}
