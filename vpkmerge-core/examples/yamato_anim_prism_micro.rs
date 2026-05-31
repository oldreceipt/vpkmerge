// Yamato animated prism pass.
//
// Input should be the Prism V3.1 particle VPK. This writes an override VPK for
// high-visibility Yamato glow/beam/trail/arc/slash particles, using only
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

const TARGET_KEYWORDS: &[&str] = &[
    "arc", "beam", "charge", "core", "dash", "endcap", "energy", "flash", "glow", "light", "magic",
    "pulse", "ring", "rope", "slash", "streak", "sweep", "tracer", "trail",
];

const SKIP_KEYWORDS: &[&str] = &[
    "blood",
    "darkness",
    "debris",
    "dust",
    "fog",
    "gas",
    "pnt",
    // power_slash is no longer skipped wholesale: its beams/trails animate fine, and
    // its sprite-sheet inputs (the square that broke it) are now skipped per-input by
    // vpkmerge_core::non_tiling_texture_inputs, not by name.
    "shake",
    "sleep",
    "smoke",
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

/// Whether `path` lies under one of the `m_Renderers[i]/m_vecTexturesInput[j]`
/// prefixes whose UV offset must not be animated (a non-tiling sprite-sheet input).
fn under_any(path: &[Seg], prefixes: &[Vec<Seg>]) -> bool {
    prefixes
        .iter()
        .any(|p| path.len() >= p.len() && path[..p.len()] == p[..])
}

fn collect_plan(v: &Value, path: &mut Vec<Seg>, skip_offset: &[Vec<Seg>], plan: &mut EntryPlan) {
    let label = path_label(path);
    let lower = label.to_ascii_lowercase();

    // Offset enum redirect + multiplier bump are UV-scroll edits: skip them on
    // non-tiling sprite-sheet inputs (they would reveal an atlas-cell square). The
    // gradient-stop retiming below is color timing, not UV, so it is unconstrained.
    let offset_animatable = !under_any(path, skip_offset);

    if offset_animatable
        && matches!(v, Value::String(s) if s != AGE_TYPE)
        && lower.contains("/m_texturecontrols/")
        && lower.contains("/m_flfinaltextureoffset")
        && lower.ends_with("/m_ntype")
    {
        plan.string_paths.push(path.clone());
    }

    if offset_animatable
        && lower.contains("/m_texturecontrols/")
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
                collect_plan(item, path, skip_offset, plan);
                path.pop();
            }
        }
        Value::Object(pairs) => {
            for (key, child) in pairs {
                path.push(Seg::Key(key.clone()));
                collect_plan(child, path, skip_offset, plan);
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
    // Texture inputs whose UV offset must NOT be animated (non-tiling sprite sheets:
    // the Power Slash square). The shared core classifier, so the example and the
    // future core animation pass agree on which inputs are safe.
    let skip_offset = vpkmerge_core::non_tiling_texture_inputs(&tree);
    let mut plan = EntryPlan::default();
    collect_plan(&tree, &mut Vec::new(), &skip_offset, &mut plan);

    let mut out = bytes.to_vec();
    let mut stats = EntryStats::default();

    // Bisection toggles: set any of these to "1" to drop that edit family, to
    // isolate which one breaks Power Slash.
    let no_offset = std::env::var("ANIM_NO_OFFSET").as_deref() == Ok("1");
    let no_gradient = std::env::var("ANIM_NO_GRADIENT").as_deref() == Ok("1");

    if !no_offset && has_string(&tree, AGE_TYPE) {
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

    if !no_offset {
        for path in plan.mult_paths {
            let (patched, ok) = patch_one_number(out, path, 2.5);
            out = patched;
            if ok {
                stats.multipliers += 1;
            }
        }
    }

    if !no_gradient {
        for (path, value) in plan.gradient_paths {
            let (patched, ok) = patch_one_number(out, path, value);
            out = patched;
            if ok {
                stats.gradients += 1;
            }
        }
    }

    Ok((out, stats))
}

fn is_target_entry(entry: &str) -> bool {
    if !(entry.starts_with("particles/abilities/yamato/")
        || entry.starts_with("particles/weapon_fx/yamato/"))
        || !entry.ends_with(".vpcf_c")
    {
        return false;
    }

    let name = entry.to_ascii_lowercase();
    TARGET_KEYWORDS.iter().any(|kw| name.contains(kw))
        && !SKIP_KEYWORDS.iter().any(|kw| name.contains(kw))
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("yamato_prism_v31_particles_dir.vpk");
    let out = args.next().expect("out_micro_dir.vpk");

    let vpk = valve_pak::open(&input)?;
    let mut entries: Vec<String> = vpk
        .file_paths()
        .filter(|entry| is_target_entry(entry))
        .cloned()
        .collect();
    entries.sort();

    let mut packed = Vec::new();
    let mut missing = 0usize;
    let mut totals = EntryStats::default();

    for entry in &entries {
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
            packed.push((entry.clone(), patched));
        }
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(entry, bytes)| (entry.as_str(), bytes.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {} patched of {} target entries, {missing} missing, {} string enum edits, {} multiplier edits, {} gradient timing edits",
        refs.len(),
        entries.len(),
        totals.strings,
        totals.multipliers,
        totals.gradients
    );
    Ok(())
}
