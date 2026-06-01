// Audit hero roster metadata and ability-VFX entry counts from a base Deadlock VPK.
//
// Usage:
//   cargo run -p vpkmerge-core --example hero_roster_audit -- <pak01_dir.vpk>

use morphic::kv3::Value;
use std::collections::BTreeSet;

fn field<'a>(obj: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn as_bool(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

fn as_i64(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Int(n)) => Some(*n),
        Some(Value::UInt(n)) => i64::try_from(*n).ok(),
        _ => None,
    }
}

fn as_str(v: Option<&Value>) -> &str {
    match v {
        Some(Value::String(s)) => s,
        _ => "",
    }
}

fn hero_label_from_logo(logo: &str) -> String {
    logo.rsplit('/')
        .next()
        .unwrap_or(logo)
        .strip_suffix("_localized.svg")
        .or_else(|| logo.rsplit('/').next().unwrap_or(logo).strip_suffix(".svg"))
        .unwrap_or("")
        .to_string()
}

fn count_prefix(vpk: &valve_pak::VPK, prefix: &str, suffix: &str) -> usize {
    vpk.file_paths()
        .filter(|p| p.starts_with(prefix) && p.ends_with(suffix))
        .count()
}

fn first_model_codename(model: &str) -> String {
    for marker in ["models/heroes_staging/", "models/heroes_wip/"] {
        if let Some(rest) = model.strip_prefix(marker) {
            return rest.split('/').next().unwrap_or("").to_string();
        }
    }
    String::new()
}

fn particle_codename(record: &str, model: &str) -> String {
    match record {
        "hero_atlas" => "abrams".to_string(),
        "hero_forge" => "mcginnis".to_string(),
        "hero_ghost" => "ghost".to_string(),
        "hero_gigawatt" => "gigawatt".to_string(),
        "hero_krill" => "digger".to_string(),
        "hero_orion" => "archer".to_string(),
        "hero_synth" => "pocket".to_string(),
        "hero_tengu" => "tengu".to_string(),
        _ => {
            let code = first_model_codename(model);
            code.trim_end_matches("_v2")
                .trim_end_matches("_v3")
                .trim_end_matches("_v4")
                .to_string()
        }
    }
}

fn main() -> anyhow::Result<()> {
    let vpk_path = std::env::args()
        .nth(1)
        .expect("usage: hero_roster_audit <pak01_dir.vpk>");
    let vpk = valve_pak::open(&vpk_path)?;
    let bytes = vpk.get_file("scripts/heroes.vdata_c")?.read_all()?;
    let root = morphic::decode_kv3_resource(&bytes)?;
    let Value::Object(top) = root else {
        anyhow::bail!("scripts/heroes.vdata_c root is not an object");
    };

    let mut ability_dirs: BTreeSet<String> = BTreeSet::new();
    let mut weapon_dirs: BTreeSet<String> = BTreeSet::new();
    for path in vpk.file_paths() {
        if let Some(rest) = path.strip_prefix("particles/abilities/") {
            if let Some(code) = rest.split('/').next() {
                if !code.contains('.') {
                    ability_dirs.insert(code.to_string());
                }
            }
        }
        if let Some(rest) = path.strip_prefix("particles/weapon_fx/") {
            if let Some(code) = rest.split('/').next() {
                if !code.contains('.') {
                    weapon_dirs.insert(code.to_string());
                }
            }
        }
    }

    println!(
        "{:<24} {:<16} {:<18} {:>4} {:>3} {:>3} {:>3} {:>3} {:>5} {:>5}  model",
        "record", "code", "label", "id", "sel", "dis", "dev", "pre", "abil", "wpn"
    );
    for (record, value) in top {
        if !record.starts_with("hero_") || record == "hero_base" || record == "hero_testhero" {
            continue;
        }
        let Value::Object(obj) = value else {
            continue;
        };
        let model = as_str(field(&obj, "m_strModelName"));
        let code = particle_codename(&record, model);
        let particle_code = if code.is_empty() {
            record.strip_prefix("hero_").unwrap_or(&record).to_string()
        } else {
            code
        };
        let label = hero_label_from_logo(as_str(field(&obj, "m_strLogoImageEnglish")));
        let abilities = count_prefix(
            &vpk,
            &format!("particles/abilities/{particle_code}/"),
            ".vpcf_c",
        );
        let weapon = count_prefix(
            &vpk,
            &format!("particles/weapon_fx/{particle_code}/"),
            ".vpcf_c",
        );
        println!(
            "{:<24} {:<16} {:<18} {:>4} {:>3} {:>3} {:>3} {:>3} {:>5} {:>5}  {}",
            record,
            particle_code,
            label,
            as_i64(field(&obj, "m_HeroID")).unwrap_or(-1),
            u8::from(as_bool(field(&obj, "m_bPlayerSelectable"))),
            u8::from(as_bool(field(&obj, "m_bDisabled"))),
            u8::from(as_bool(field(&obj, "m_bInDevelopment"))),
            u8::from(as_bool(field(&obj, "m_bPrereleaseOnly"))),
            abilities,
            weapon,
            model,
        );
    }

    let mut all_particle_dirs = ability_dirs;
    all_particle_dirs.extend(weapon_dirs);
    eprintln!("particle hero dirs: {}", all_particle_dirs.len());
    Ok(())
}
