// Yamato animated prism micro-pass.
//
// Input should be the Prism V3.1 particle VPK. This writes a small override VPK
// for selected high-visibility Yamato glow/beam/trail particles, using only
// byte-faithful existing-field patches:
//   - texture offset inputs -> particle age, when that enum string is present
//   - stored texture-offset multipliers -> 2.5
//   - existing gradient stop positions tightened for earlier spectral changes
//
// usage:
//   cargo run -p vpkmerge-core --example yamato_anim_prism_micro -- \
//     <yamato_prism_v31_particles_dir.vpk> <out_micro_dir.vpk>
use morphic::kv3::{Seg, Value};

const AGE_TYPE: &str = "PF_TYPE_PARTICLE_AGE_NORMALIZED";

const TARGETS: &[&str] = &[
    "particles/abilities/yamato/yamato_power_slash_charge_body_glow.vpcf_c",
    "particles/abilities/yamato/yamato_power_slash_charge_magic.vpcf_c",
    "particles/abilities/yamato/yamato_power_slash_charge_light.vpcf_c",
    "particles/abilities/yamato/yamato_power_slash_charge_streaks.vpcf_c",
    "particles/abilities/yamato/yamato_power_slash_pulse_glow.vpcf_c",
    "particles/abilities/yamato/yamato_power_slash_sweep_line.vpcf_c",
    "particles/abilities/yamato/yamato_blade_glow_trail.vpcf_c",
    "particles/abilities/yamato/yamato_blade_dash_trail.vpcf_c",
    "particles/abilities/yamato/yamato_blade_dash_trail_core.vpcf_c",
    "particles/abilities/yamato/yamato_blade_dash_model_glow.vpcf_c",
    "particles/abilities/yamato/yamato_crimson_slash_blade_glow_beam.vpcf_c",
    "particles/abilities/yamato/yamato_crimson_slash_blade_glow_trail.vpcf_c",
    "particles/abilities/yamato/yamato_infinity_slash_start_beam.vpcf_c",
    "particles/abilities/yamato/yamato_infinity_slash_end_beam.vpcf_c",
    "particles/abilities/yamato/yamato_infinity_slash_dash_trail.vpcf_c",
    "particles/abilities/yamato/yamato_shadow_form_glow.vpcf_c",
    "particles/abilities/yamato/yamato_shadow_form_energy.vpcf_c",
    "particles/abilities/yamato/yamato_shadow_form_source_beam.vpcf_c",
    "particles/abilities/yamato/yamato_flying_strike_rope_tracer_glow.vpcf_c",
    "particles/abilities/yamato/yamato_flying_strike_rope_tracer_trail.vpcf_c",
    "particles/weapon_fx/yamato/yamato_weapon_arc_beam.vpcf_c",
    "particles/weapon_fx/yamato/yamato_weapon_arc_trail.vpcf_c",
    "particles/weapon_fx/yamato/yamato_tracer_track_trail.vpcf_c",
];

#[derive(Default)]
struct EntryPlan {
    string_paths: Vec<Vec<Seg>>,
    mult_paths: Vec<Vec<Seg>>,
    gradient_paths: Vec<(Vec<Seg>, f64)>,
}

#[derive(Default)]
struct EntryStats {
    strings: usize,
    multipliers: usize,
    gradients: usize,
}

fn has_string(v: &Value, needle: &str) -> bool {
    match v {
        Value::String(s) => s == needle,
        Value::Array(items) => items.iter().any(|v| has_string(v, needle)),
        Value::Object(pairs) => pairs.iter().any(|(_, v)| has_string(v, needle)),
        _ => false,
    }
}

fn path_label(path: &[Seg]) -> String {
    let mut out = String::new();
    for seg in path {
        match seg {
            Seg::Key(k) => {
                out.push('/');
                out.push_str(k);
            }
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

fn stop_target(label: &str) -> Option<f64> {
    if label.contains("/m_Stops[1]/m_flPosition") {
        Some(0.18)
    } else if label.contains("/m_Stops[2]/m_flPosition") {
        Some(0.45)
    } else if label.contains("/m_Stops[3]/m_flPosition") {
        Some(0.72)
    } else {
        None
    }
}

fn collect_plan(v: &Value, path: &mut Vec<Seg>, plan: &mut EntryPlan) {
    let label = path_label(path);
    let lower = label.to_ascii_lowercase();

    if matches!(v, Value::String(s) if s != AGE_TYPE)
        && lower.contains("/m_texturecontrols/")
        && lower.contains("/m_flfinaltextureoffset")
        && lower.ends_with("/m_ntype")
    {
        plan.string_paths.push(path.clone());
    }

    if lower.contains("/m_texturecontrols/")
        && lower.contains("/m_flfinaltextureoffset")
        && lower.ends_with("/m_flmultfactor")
    {
        plan.mult_paths.push(path.clone());
    }

    if lower.contains("/m_gradient/m_stops") && lower.ends_with("/m_flposition") {
        if let Some(target) = stop_target(&label) {
            plan.gradient_paths.push((path.clone(), target));
        }
    }

    match v {
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Seg::Index(i));
                collect_plan(item, path, plan);
                path.pop();
            }
        }
        Value::Object(pairs) => {
            for (key, child) in pairs {
                path.push(Seg::Key(key.clone()));
                collect_plan(child, path, plan);
                path.pop();
            }
        }
        _ => {}
    }
}

fn patch_one_number(bytes: Vec<u8>, path: Vec<Seg>, value: f64) -> (Vec<u8>, bool) {
    if let Ok(patched) = morphic::patch_kv3_resource_doubles(&bytes, &[(path.clone(), value)]) {
        return (patched, true);
    }
    if let Ok(patched) = morphic::patch_kv3_resource_floats(&bytes, &[(path, value as f32)]) {
        return (patched, true);
    }
    (bytes, false)
}

fn patch_entry(bytes: &[u8]) -> anyhow::Result<(Vec<u8>, EntryStats)> {
    let tree = morphic::decode_kv3_resource(bytes)?;
    let mut plan = EntryPlan::default();
    collect_plan(&tree, &mut Vec::new(), &mut plan);

    let mut out = bytes.to_vec();
    let mut stats = EntryStats::default();

    if has_string(&tree, AGE_TYPE) {
        for path in plan.string_paths {
            match morphic::patch_kv3_resource_strings(&out, &[(path, AGE_TYPE.to_string())]) {
                Ok(patched) => {
                    out = patched;
                    stats.strings += 1;
                }
                Err(_) => {}
            }
        }
    }

    for path in plan.mult_paths {
        let (patched, ok) = patch_one_number(out, path, 2.5);
        out = patched;
        if ok {
            stats.multipliers += 1;
        }
    }

    for (path, value) in plan.gradient_paths {
        let (patched, ok) = patch_one_number(out, path, value);
        out = patched;
        if ok {
            stats.gradients += 1;
        }
    }

    Ok((out, stats))
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("yamato_prism_v31_particles_dir.vpk");
    let out = args.next().expect("out_micro_dir.vpk");

    let vpk = valve_pak::open(&input)?;
    let mut packed = Vec::new();
    let mut missing = 0usize;
    let mut totals = EntryStats::default();

    for &entry in TARGETS {
        let Ok(mut file) = vpk.get_file(entry) else {
            missing += 1;
            eprintln!("missing {entry}");
            continue;
        };
        let bytes = file.read_all()?;
        let (patched, stats) = patch_entry(&bytes)?;
        totals.strings += stats.strings;
        totals.multipliers += stats.multipliers;
        totals.gradients += stats.gradients;
        if stats.strings + stats.multipliers + stats.gradients > 0 {
            packed.push((entry.to_string(), patched));
        }
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(entry, bytes)| (entry.as_str(), bytes.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {} entries, {missing} missing, {} string enum edits, {} multiplier edits, {} gradient timing edits",
        refs.len(),
        totals.strings,
        totals.multipliers,
        totals.gradients
    );
    Ok(())
}
