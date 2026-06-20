//! Build the Foundry voice-line search index from a Deadlock VPK.
//!
//! Joins the compiled VO caption database against the `soundevents/vo` tree and
//! prints one `{ event, hero, text }` row per spoken line (those with English
//! subtitle text). The base `citadel/pak01_dir.vpk` carries the English captions.
//!
//! Usage:
//!   cargo run -p vpkmerge-core --example voiceline_index -- <citadel_pak01_dir.vpk> [--json]

use vpkmerge_core::build_voiceline_index;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("usage: voiceline_index <citadel_pak01_dir.vpk> [--json]");
            std::process::exit(2);
        });
    let json = args.iter().any(|a| a == "--json");

    let lines = build_voiceline_index(&vpk)?;

    if json {
        let arr: Vec<serde_json::Value> = lines
            .iter()
            .map(|l| {
                serde_json::json!({
                    "event": l.event,
                    "hero": l.hero,
                    "label": l.label,
                    "vsnd": l.vsnd,
                    "duration": l.duration,
                    "caption": l.caption,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        for l in &lines {
            println!(
                "{:<12} {:<52} {}",
                l.hero.as_deref().unwrap_or("-"),
                l.event,
                l.caption.as_deref().unwrap_or(&l.label)
            );
        }
    }
    let with_caption = lines.iter().filter(|l| l.caption.is_some()).count();
    eprintln!(
        "{} VO events ({with_caption} with authored captions)",
        lines.len()
    );
    Ok(())
}
