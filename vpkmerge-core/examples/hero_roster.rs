//! Print the Deadlock hero roster with display names.
//!
//! Reads the codenames + availability flags from `scripts/heroes.vdata_c` inside
//! the pak and resolves each `hero_<codename>` token against the loose
//! `resource/localization/citadel_gc_hero_names_<lang>.txt` next to the pak. This
//! is the codename -> display-name table the Foundry catalog uses to label its
//! codename-keyed rows.
//!
//! Usage:
//!   cargo run -p vpkmerge-core --example hero_roster -- <citadel_pak01_dir.vpk> [lang]

use vpkmerge_core::build_hero_roster;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = args.get(1).cloned().unwrap_or_else(|| {
        eprintln!("usage: hero_roster <citadel_pak01_dir.vpk> [lang]");
        std::process::exit(2);
    });
    let lang = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| vpkmerge_core::DEFAULT_LANG.to_owned());

    let roster = build_hero_roster(&vpk, None, &lang)?;

    let selectable = roster
        .iter()
        .filter(|h| h.selectable && !h.disabled)
        .count();
    for h in &roster {
        let mut tags = Vec::new();
        if h.selectable {
            tags.push("selectable");
        }
        if h.in_development {
            tags.push("in-dev");
        }
        if h.disabled {
            tags.push("disabled");
        }
        println!("{:<14} {:<18} {}", h.codename, h.name, tags.join(", "));
    }
    eprintln!("{} heroes ({selectable} selectable)", roster.len());
    Ok(())
}
