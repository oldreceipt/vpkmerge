// Condensed structural outline of a Source 2 particle system (`.vpcf_c`). Lists
// each emitter / initializer / operator / renderer by class with only its
// meaningful (non-default) params, collapsing the ~60-field
// PF_TYPE_LITERAL / PVEC_TYPE_LITERAL input wrappers down to their effective
// value, follows `m_Children` through the VPK, and finishes with a whole-tree
// operator-class histogram tagged by how cleanly each class maps onto a Blender
// particle / geometry-nodes rebuild.
//
// Built to scope the "render Deadlock ability VFX in Blender instead of
// in-engine" pipeline (morphic decode -> Blender MCP): run it on one ability and
// read the operator inventory + the tag table to see whether the translator is
// a tidy ~10-operator mapping or a per-effect slog.
//
// usage:
//   cargo run --release -p vpkmerge-core --example vpcf_outline -- \
//       --from-vpk <pak01_dir.vpk> particles/abilities/yamato/yamato_crimson_slash.vpcf_c
//   cargo run --release -p vpkmerge-core --example vpcf_outline -- some_loose_file.vpcf_c
//
// flags:
//   --flat          do not follow m_Children
//   --all           show default / zero-valued fields too (default: hide them)
//   --max-depth N   child recursion depth (default 6)
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use morphic::kv3::Value;
use std::collections::{BTreeMap, HashSet};

/// Operator buckets, printed in this order. These are the arrays the engine runs
/// each frame; everything else on the system is plumbing we ignore.
const SECTIONS: &[(&str, &str)] = &[
    ("m_PreEmissionOperators", "pre-emission"),
    ("m_Emitters", "emitters"),
    ("m_Initializers", "initializers"),
    ("m_Operators", "operators"),
    ("m_ForceGenerators", "forces"),
    ("m_Constraints", "constraints"),
    ("m_Renderers", "renderers"),
];

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Double(d) => Some(*d),
        _ => None,
    }
}

/// Trim trailing `.0` so `85.0` prints as `85` but `0.1` stays `0.1`.
fn fnum(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

fn vecbrief(a: &[Value]) -> String {
    let parts: Vec<String> = a
        .iter()
        .map(|x| num(x).map_or_else(|| "?".into(), fnum))
        .collect();
    format!("[{}]", parts.join(", "))
}

/// If `v` is a `CParticleFloatInput` / `CParticleVecInput` wrapper, collapse it
/// to a short readable string (the literal, or `rand(a..b)`, or the source kind
/// plus any gradient / curve remap). Otherwise `None`.
fn input_brief(v: &Value) -> Option<String> {
    let ty = v.get("m_nType")?.as_str()?;
    let is_vec = ty.starts_with("PVEC_TYPE_");
    if !is_vec && !ty.starts_with("PF_TYPE_") {
        return None;
    }
    let mut s = if ty.ends_with("LITERAL") {
        if is_vec {
            v.get("m_vLiteralValue")
                .and_then(Value::as_array)
                .map_or_else(|| "0".into(), |a| vecbrief(a))
        } else {
            fnum(v.get("m_flLiteralValue").and_then(num).unwrap_or(0.0))
        }
    } else {
        let short = ty
            .trim_start_matches("PF_TYPE_")
            .trim_start_matches("PVEC_TYPE_")
            .to_lowercase();
        if short.contains("random") {
            if is_vec {
                let lo = v
                    .get("m_vRandomMin")
                    .and_then(Value::as_array)
                    .map_or_else(String::new, |a| vecbrief(a));
                let hi = v
                    .get("m_vRandomMax")
                    .and_then(Value::as_array)
                    .map_or_else(String::new, |a| vecbrief(a));
                format!("rand({lo}..{hi})")
            } else {
                let lo = fnum(v.get("m_flRandomMin").and_then(num).unwrap_or(0.0));
                let hi = fnum(v.get("m_flRandomMax").and_then(num).unwrap_or(0.0));
                format!("rand({lo}..{hi})")
            }
        } else if short.contains("control_point") {
            match v.get("m_nControlPoint").and_then(Value::as_int) {
                Some(c) => format!("{short}#{c}"),
                None => short,
            }
        } else {
            short
        }
    };
    if let Some(stops) = v
        .get("m_Gradient")
        .and_then(|g| g.get("m_Stops"))
        .and_then(Value::as_array)
    {
        if !stops.is_empty() {
            s = format!("{s} grad[{} stops]", stops.len());
        }
    }
    if let Some(sp) = v
        .get("m_Curve")
        .and_then(|c| c.get("m_spline"))
        .and_then(Value::as_array)
    {
        if !sp.is_empty() {
            s = format!("{s} curve[{}]", sp.len());
        }
    }
    Some(s)
}

/// One-line summary of a value. Objects collapse to an input brief, a gradient
/// brief, or `{N fields}`; numeric arrays up to length 4 print inline.
fn brief(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Double(d) => fnum(*d),
        Value::String(s) => format!("{s:?}"),
        Value::Binary(b) => format!("<{} bytes>", b.len()),
        Value::Array(a) => {
            if !a.is_empty() && a.iter().all(|x| num(x).is_some()) && a.len() <= 4 {
                vecbrief(a)
            } else {
                format!("[{} items]", a.len())
            }
        }
        Value::Object(pairs) => input_brief(v)
            .or_else(|| {
                v.get("m_Stops")
                    .and_then(Value::as_array)
                    .map(|s| format!("grad[{} stops]", s.len()))
            })
            .unwrap_or_else(|| format!("{{{} fields}}", pairs.len())),
    }
}

/// Fields whose brief carries no signal: empty, zero, or false.
fn boring(s: &str) -> bool {
    matches!(
        s,
        "" | "0"
            | "0.0"
            | "[]"
            | "{}"
            | "false"
            | "\"\""
            | "[0, 0]"
            | "[0, 0, 0]"
            | "[0, 0, 0, 0]"
            | "{0 fields}"
    )
}

fn asset_leaf(s: &str) -> String {
    s.rsplit('/').next().unwrap_or(s).to_string()
}

/// Shallow-walk a renderer entry for the sprite/material it draws with: the one
/// asset a Blender rebuild needs to fetch for this renderer.
fn collect_assets(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(pairs) => {
            for (k, val) in pairs {
                if matches!(
                    k.as_str(),
                    "m_hTexture" | "m_hMaterial" | "m_hOverrideMaterial" | "m_hModel"
                ) {
                    if let Some(s) = val.as_str() {
                        if !s.is_empty() {
                            let leaf = asset_leaf(s);
                            if !out.contains(&leaf) {
                                out.push(leaf);
                            }
                        }
                    }
                }
                collect_assets(val, out);
            }
        }
        Value::Array(a) => {
            for x in a {
                collect_assets(x, out);
            }
        }
        _ => {}
    }
}

fn print_op(
    v: &Value,
    indent: &str,
    is_renderer: bool,
    all: bool,
    hist: &mut BTreeMap<String, u32>,
) {
    let class = v
        .get("_class")
        .and_then(Value::as_str)
        .unwrap_or("<no _class>");
    *hist.entry(class.to_string()).or_default() += 1;

    let mut fields = Vec::new();
    let mut hidden = 0u32;
    if let Value::Object(pairs) = v {
        for (k, val) in pairs {
            if k == "_class" {
                continue;
            }
            let b = brief(val);
            if !all && boring(&b) {
                hidden += 1;
                continue;
            }
            fields.push(format!("{}={}", k.trim_start_matches("m_"), b));
        }
    }
    if is_renderer {
        let mut assets = Vec::new();
        collect_assets(v, &mut assets);
        if !assets.is_empty() {
            fields.push(format!("draws={{{}}}", assets.join(", ")));
        }
    }

    print!("{indent}- {class}");
    if !fields.is_empty() {
        print!("   {}", fields.join("  "));
    }
    if hidden > 0 && all {
        print!("   (+{hidden} default)");
    }
    println!();
}

fn preview_model(v: &Value) -> Option<String> {
    v.get("m_controlPointConfigurations")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|cfg| {
            cfg.get("m_previewState")
                .and_then(|p| p.get("m_previewModel"))
                .and_then(Value::as_str)
                .map(asset_leaf)
        })
}

/// `.vpcf` child ref -> compiled `.vpcf_c` VPK entry.
fn compiled(reference: &str) -> String {
    if reference.ends_with("_c") {
        reference.to_string()
    } else {
        format!("{reference}_c")
    }
}

struct Cfg {
    flat: bool,
    all: bool,
    max_depth: u8,
}

fn outline(
    label: &str,
    bytes: &[u8],
    depth: u8,
    vpk: Option<&valve_pak::VPK>,
    cfg: &Cfg,
    hist: &mut BTreeMap<String, u32>,
    visited: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let indent = "  ".repeat(depth as usize);
    let v = morphic::decode_kv3_resource(bytes)?;
    let class = v.get("_class").and_then(Value::as_str).unwrap_or("?");
    let max = v
        .get("m_nMaxParticles")
        .and_then(Value::as_int)
        .map_or_else(|| "?".into(), |n| n.to_string());

    println!("{indent}# {}   [{class}]  max={max}", asset_leaf(label));
    if let Some(m) = preview_model(&v) {
        println!("{indent}    preview-model: {m}");
    }

    for (key, sec_label) in SECTIONS {
        let Some(arr) = v.get(key).and_then(Value::as_array) else {
            continue;
        };
        if arr.is_empty() {
            continue;
        }
        println!("{indent}  {sec_label} ({}):", arr.len());
        let is_renderer = *key == "m_Renderers";
        for op in arr {
            print_op(op, &format!("{indent}    "), is_renderer, cfg.all, hist);
        }
    }

    if cfg.flat || depth >= cfg.max_depth {
        return Ok(());
    }
    let Some(kids) = v.get("m_Children").and_then(Value::as_array) else {
        return Ok(());
    };
    for kid in kids {
        let Some(reference) = kid.get("m_ChildRef").and_then(Value::as_str) else {
            continue;
        };
        let entry = compiled(reference);
        let leaf = asset_leaf(&entry);
        if visited.contains(&entry) {
            println!("{indent}  -> child (shown above): {leaf}");
            continue;
        }
        visited.insert(entry.clone());
        let Some(vpk) = vpk else {
            println!("{indent}  -> child (needs --from-vpk to follow): {leaf}");
            continue;
        };
        match vpk.get_file(&entry).and_then(|mut f| f.read_all()) {
            Ok(b) => outline(&entry, &b, depth + 1, Some(vpk), cfg, hist, visited)?,
            Err(e) => println!("{indent}  -> child UNRESOLVED {leaf}: {e}"),
        }
    }
    Ok(())
}

/// (tag, note). Tags order the final table; the note explains the Blender mapping.
fn tag_for(c: &str) -> (&'static str, &'static str) {
    match c {
        "C_OP_ContinuousEmitter" => ("easy", "emission rate over time"),
        "C_OP_InstantaneousEmitter" => ("easy", "single burst at spawn"),
        "C_OP_Decay" | "C_OP_DecayClamp" => ("easy", "kill at end of lifespan"),
        "C_OP_FadeOut" | "C_OP_FadeIn" | "C_OP_FadeInSimple" | "C_OP_FadeOutSimple"
        | "C_OP_FadeAndKill" => ("easy", "alpha over life"),
        "C_OP_BasicMovement" => ("easy", "velocity + gravity + drag integration"),
        "C_OP_InterpolateRadius" => ("easy", "size (radius) over life"),
        "C_OP_ColorInterpolate" => ("easy", "color over life (gradient) <- recolor/prism"),
        "C_OP_SpinUpdate" | "C_OP_Spin" | "C_OP_SpinYaw" => ("easy", "angular spin"),
        "C_OP_RampScalarLinearSimple" | "C_OP_RampScalarLinear" => {
            ("easy", "linear ramp of an attribute over life")
        }
        "C_OP_PositionLock" | "C_OP_PositionLockToControlPoint" => {
            ("easy", "follow emitter / control point")
        }
        "C_OP_MovementRotateParticleAroundAxis" => ("easy", "orbit / rotate around an axis"),
        "C_OP_RenderScreenShake" => ("plumbing", "camera shake, not a visual (drop)"),
        "C_INIT_RemapInitialDirectionToTransformToVector" => {
            ("shape", "spawn direction -> vector attribute")
        }
        "C_INIT_InitFloat" | "C_INIT_InitVec" => ("easy", "init attribute (needs field-id map)"),
        "C_INIT_RandomColor" => ("easy", "random initial color"),
        "C_INIT_PositionOffset" => ("easy", "spawn position offset"),
        "C_INIT_RandomSequence" => ("shape", "random sprite-sheet frame"),
        "C_INIT_RingWave" => ("shape", "ring / wave emission shape"),
        "C_INIT_NormalAlignToCP" => ("shape", "align particle normal to a CP"),
        "C_INIT_RemapInitialTransformDirectionToRotation" => {
            ("shape", "spawn direction -> rotation")
        }
        "C_INIT_InitialVelocityNoise" => ("approx", "noise-field initial velocity"),
        "C_OP_RemapSpeed" => ("approx", "drive attribute from speed"),
        "C_OP_RenderSprites" => ("render", "camera-facing billboard quad (sprite)"),
        "C_OP_RenderRopes" => ("render", "ribbon/trail: build a curve with profile"),
        "C_OP_RenderTrails" => ("render", "trail: build a curve with profile"),
        "C_OP_RenderProjected" => ("render", "projected decal (approximate as a plane)"),
        "C_OP_RenderModels" => ("render", "mesh instances (reuse `vpkmerge model export`)"),
        _ => fallback_tag(c),
    }
}

fn fallback_tag(c: &str) -> (&'static str, &'static str) {
    if c.starts_with("C_INIT_Create") {
        ("shape", "emission geometry")
    } else if c.contains("Sequence") {
        ("shape", "sprite-sheet frame")
    } else if c.contains("AlignTo")
        || c.contains("Orientation")
        || c.contains("Rotation")
        || c.contains("Normal")
        || c.contains("Yaw")
    {
        ("shape", "orientation / rotation init")
    } else if c.contains("Velocity") || c.contains("Speed") {
        ("easy", "velocity")
    } else if c.contains("Offset") || c.contains("PositionWarp") {
        ("easy", "spawn position")
    } else if c.starts_with("C_OP_Render") {
        ("render", "renderer (inspect)")
    } else if c.contains("Force")
        || c.contains("Attract")
        || c.contains("Twist")
        || c.contains("Vortex")
        || c.contains("Drag")
    {
        ("force", "force field (approximate)")
    } else if c.contains("ControlPoint") {
        ("plumbing", "control-point setup (just place the emitter)")
    } else if c.contains("Ramp") || c.contains("Lerp") {
        ("easy", "ramp an attribute over life")
    } else if c.contains("Rotate") || c.contains("Movement") || c.contains("Lock") {
        ("easy", "motion / movement")
    } else if c.starts_with("C_OP_Set") || c.starts_with("C_INIT_") {
        ("easy", "set / init attribute")
    } else if c.contains("Remap") || c.contains("Oscillate") || c.contains("Noise") {
        ("approx", "remap / oscillate / noise")
    } else {
        ("?", "not yet classified (inspect)")
    }
}

const TAG_ORDER: &[(&str, &str)] = &[
    ("easy", "direct Blender particle/geo-nodes concept"),
    (
        "shape",
        "emission geometry or orientation -> emitter mesh / init",
    ),
    (
        "render",
        "renderer-specific build (billboard / ribbon / decal / mesh)",
    ),
    ("force", "force field, approximate"),
    ("approx", "noise / remap / oscillate, approximate"),
    (
        "plumbing",
        "engine bookkeeping, no Blender equivalent (skip)",
    ),
    ("?", "unclassified, needs inspection"),
];

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Cfg {
        flat: false,
        all: false,
        max_depth: 6,
    };
    let mut vpk_path: Option<String> = None;

    if let Some(i) = args.iter().position(|a| a == "--from-vpk") {
        vpk_path = args.get(i + 1).cloned();
        args.drain(i..=i + 1);
    }
    if let Some(i) = args.iter().position(|a| a == "--max-depth") {
        cfg.max_depth = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(6);
        args.drain(i..=i + 1);
    }
    args.retain(|a| {
        match a.as_str() {
            "--flat" => cfg.flat = true,
            "--all" => cfg.all = true,
            _ => return true,
        }
        false
    });

    let Some(target) = args.first().cloned() else {
        eprintln!(
            "usage: vpcf_outline [--from-vpk <vpk>] [--flat] [--all] [--max-depth N] <entry-or-file.vpcf_c>"
        );
        std::process::exit(2);
    };

    let vpk = match &vpk_path {
        Some(p) => Some(valve_pak::open(p)?),
        None => None,
    };
    let root_bytes = match &vpk {
        Some(v) => v.get_file(&target).and_then(|mut f| f.read_all())?,
        None => std::fs::read(&target)?,
    };

    let mut hist: BTreeMap<String, u32> = BTreeMap::new();
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(compiled(&target));

    println!("===== particle tree: {} =====\n", asset_leaf(&target));
    outline(
        &target,
        &root_bytes,
        0,
        vpk.as_ref(),
        &cfg,
        &mut hist,
        &mut visited,
    )?;

    // operator histogram, grouped by Blender-mapping difficulty
    let total: u32 = hist.values().sum();
    let distinct = hist.len();
    println!("\n===== operator-class inventory ({distinct} distinct, {total} instances) =====");
    let mut tagged: Vec<(&str, &str, &str, u32)> = hist
        .iter()
        .map(|(c, n)| {
            let (tag, note) = tag_for(c);
            (tag, c.as_str(), note, *n)
        })
        .collect();
    for (tag, legend) in TAG_ORDER {
        let mut group: Vec<&(&str, &str, &str, u32)> =
            tagged.iter().filter(|t| t.0 == *tag).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| b.3.cmp(&a.3).then(a.1.cmp(b.1)));
        let group_total: u32 = group.iter().map(|t| t.3).sum();
        println!("\n  [{tag}] {legend}  ({group_total} instances)");
        for (_, class, note, n) in group {
            println!("    x{n:<3} {class:<46} {note}");
        }
    }
    // keep the borrow checker happy about `tagged` mutation-free use above
    tagged.clear();

    let easy: u32 = hist
        .iter()
        .filter(|(c, _)| matches!(tag_for(c).0, "easy" | "shape"))
        .map(|(_, n)| *n)
        .sum();
    println!(
        "\n  => {easy}/{total} operator instances ({:.0}%) are 'easy'+'shape' (direct or near-direct Blender mapping).",
        100.0 * f64::from(easy) / f64::from(total.max(1))
    );
    Ok(())
}
