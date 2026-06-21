//! Warm the on-disk Foundry catalog cache, then prove the second build is a hit.
//!
//! Builds the voice-line and texture indexes for a pak, caches them keyed by the
//! pak's build fingerprint (`_dir.vpk` size + mtime), and loads them straight back
//! to show the cache resolves. Run twice on an unchanged pak and the second run
//! reports both indexes as cache hits.
//!
//! Usage:
//!   cargo run -p vpkmerge-core --example catalog_cache -- <citadel_pak01_dir.vpk> [cache_dir]

use vpkmerge_core::{BuildFingerprint, CatalogCache};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let vpk = args.get(1).cloned().unwrap_or_else(|| {
        eprintln!("usage: catalog_cache <citadel_pak01_dir.vpk> [cache_dir]");
        std::process::exit(2);
    });
    let dir = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "catalog-cache".to_owned());

    let fingerprint = BuildFingerprint::for_vpk(&vpk)?;
    println!(
        "build fingerprint: {} bytes, mtime {}.{:09}",
        fingerprint.vpk_len, fingerprint.vpk_mtime_secs, fingerprint.vpk_mtime_nanos
    );

    let cache = CatalogCache::new(&dir);

    let (voicelines, vo_hit) = cache.voicelines_cached(&vpk)?;
    let (textures, tex_hit) = cache.textures_cached(&vpk)?;

    println!(
        "voiceline: {} events ({})",
        voicelines.len(),
        if vo_hit { "cache hit" } else { "rebuilt" }
    );
    println!(
        "texture:   {} entries ({})",
        textures.len(),
        if tex_hit { "cache hit" } else { "rebuilt" }
    );
    println!("cache dir: {dir}");
    Ok(())
}
