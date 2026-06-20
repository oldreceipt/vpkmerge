//! Build the Foundry texture / icon browse index from a Deadlock VPK.
//!
//! Classifies every `.vtex_c` from its path (ability icon, item icon, hero
//! portrait, hero skin texture, ability VFX) and prints a per-category count
//! plus a sample. With `--thumbs DIR` it also decodes a small PNG thumbnail for
//! each ability icon into DIR (the grid backbone for the Texture / Item tabs).
//!
//! Usage:
//!   cargo run -p vpkmerge-core --example texture_index -- <citadel_pak01_dir.vpk> [--thumbs DIR]

use std::collections::BTreeMap;

use vpkmerge_core::{
    build_texture_index, cache_texture_thumbnails, TextureCategory, ThumbnailOutcome,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("usage: texture_index <citadel_pak01_dir.vpk> [--thumbs DIR]");
            std::process::exit(2);
        });
    let thumbs_dir = args
        .iter()
        .position(|a| a == "--thumbs")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let entries = build_texture_index(&vpk)?;

    // Per-category count.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &entries {
        *counts.entry(e.category.id()).or_default() += 1;
    }
    println!("== {} textures ==", entries.len());
    for (cat, n) in &counts {
        println!("  {cat:<13} {n}");
    }

    // A small sample of ability icons.
    println!("\n== sample ability icons ==");
    for e in entries
        .iter()
        .filter(|e| e.category == TextureCategory::AbilityIcon && e.hero.is_some())
        .take(8)
    {
        println!(
            "  {:<10} {:<28} {}",
            e.hero.as_deref().unwrap_or("-"),
            e.label,
            e.path
        );
    }

    if let Some(dir) = thumbs_dir {
        let icons: Vec<_> = entries
            .into_iter()
            .filter(|e| e.category == TextureCategory::AbilityIcon)
            .collect();
        let outcomes = cache_texture_thumbnails(&vpk, &icons, &dir, 128)?;
        let cached = outcomes
            .iter()
            .filter(|o| matches!(o, ThumbnailOutcome::Cached(_)))
            .count();
        println!("\nwrote {cached} ability-icon thumbnail(s) to {dir}");
    }

    Ok(())
}
