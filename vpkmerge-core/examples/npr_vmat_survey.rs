//! Survey NPR / toon-shading usage across shipped .vmat_c materials.
//!
//! For the NPR toon-shading investigation: which F_* feature flags and NPR
//! params does shipped content actually exercise (proof the engine honors them
//! as material data), and do any materials carry float/vector ATTRIBUTES
//! (the channel the __Attribute__-sourced cel-band params would ride on)?
//!
//! usage: cargo run --release --example npr_vmat_survey -- <pak01_dir.vpk> [prefix]

use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let pak = a.next().expect("usage: npr_vmat_survey <pak> [prefix]");
    let prefix = a.next().unwrap_or_default();

    let info = vpkmerge_core::inspect(&pak)?;
    let vmats: Vec<&String> = info
        .file_paths
        .iter()
        .filter(|p| p.ends_with(".vmat_c") && p.starts_with(&prefix))
        .collect();
    eprintln!("{} .vmat_c entries under '{prefix}'", vmats.len());

    let mut flag_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut float_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut attr_materials: Vec<(String, String)> = Vec::new();
    let mut outline_override_examples: Vec<String> = Vec::new();
    let mut unlit_examples: Vec<String> = Vec::new();
    let mut sheen_examples: Vec<String> = Vec::new();
    let mut glass_examples: Vec<String> = Vec::new();
    let mut decoded = 0usize;
    let mut failed = 0usize;

    for entry in &vmats {
        let Ok(bytes) = vpkmerge_core::read_vpk_entry(&pak, entry) else {
            failed += 1;
            continue;
        };
        let Ok(v) = morphic::decode_kv3_resource(&bytes) else {
            failed += 1;
            continue;
        };
        decoded += 1;

        if let Some(morphic::kv3::Value::Array(params)) = v.get("m_intParams") {
            for p in params {
                let (Some(name), Some(val)) = (
                    p.get("m_name").and_then(morphic::kv3::Value::as_str),
                    p.get("m_nValue").and_then(morphic::kv3::Value::as_int),
                ) else {
                    continue;
                };
                if val != 0
                    && (name.contains("NPR")
                        || name.contains("OUTLINE")
                        || name == "F_UNLIT"
                        || name == "F_SELF_ILLUM"
                        || name == "F_SHEEN"
                        || name == "F_GLASS"
                        || name == "F_CLOAK"
                        || name == "F_METALNESS_TEXTURE")
                {
                    *flag_counts.entry(name.to_string()).or_default() += 1;
                    if name == "F_OVERRIDE_NPR_OUTLINE" && outline_override_examples.len() < 8 {
                        outline_override_examples.push((*entry).clone());
                    }
                    if name == "F_UNLIT" && unlit_examples.len() < 8 {
                        unlit_examples.push((*entry).clone());
                    }
                    if name == "F_SHEEN" && sheen_examples.len() < 8 {
                        sheen_examples.push((*entry).clone());
                    }
                    if name == "F_GLASS" && glass_examples.len() < 8 {
                        glass_examples.push((*entry).clone());
                    }
                }
            }
        }

        if let Some(morphic::kv3::Value::Array(params)) = v.get("m_floatParams") {
            for p in params {
                let Some(name) = p.get("m_name").and_then(morphic::kv3::Value::as_str) else {
                    continue;
                };
                if name.contains("Npr") || name.contains("NPR") || name.contains("Outline") {
                    *float_counts.entry(name.to_string()).or_default() += 1;
                }
            }
        }

        for table in ["m_floatAttributes", "m_vectorAttributes"] {
            if let Some(morphic::kv3::Value::Array(attrs)) = v.get(table) {
                for at in attrs {
                    if let Some(name) = at.get("m_name").and_then(morphic::kv3::Value::as_str) {
                        if attr_materials.len() < 40 {
                            attr_materials.push((name.to_string(), (*entry).clone()));
                        }
                    }
                }
            }
        }
    }

    println!("decoded {decoded} / {} ({} failed)", vmats.len(), failed);
    println!("\n# NPR-relevant int flags set nonzero (count of materials):");
    for (k, n) in &flag_counts {
        println!("  {k:<40} {n}");
    }
    println!("\n# NPR/outline float params present (count of materials):");
    for (k, n) in &float_counts {
        println!("  {k:<40} {n}");
    }
    println!("\n# float/vector ATTRIBUTES seen (name -> example material):");
    let mut seen = std::collections::BTreeSet::new();
    for (name, entry) in &attr_materials {
        if seen.insert(name.clone()) {
            println!("  {name:<40} {entry}");
        }
    }
    println!("\n# F_OVERRIDE_NPR_OUTLINE examples:");
    for e in &outline_override_examples {
        println!("  {e}");
    }
    println!("\n# F_UNLIT examples:");
    for e in &unlit_examples {
        println!("  {e}");
    }
    println!("\n# F_SHEEN examples:");
    for e in &sheen_examples {
        println!("  {e}");
    }
    println!("\n# F_GLASS examples:");
    for e in &glass_examples {
        println!("  {e}");
    }
    Ok(())
}
