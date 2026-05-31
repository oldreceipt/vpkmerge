// Structural-edit viability probe: identity-re-encode every .vpcf_c under the
// given prefixes (decode KV3 -> re-encode with NO semantic change) and pack them at
// their own paths. If the result loads cleanly in game, a full KV3 re-encode is
// viable for particles, which unlocks BOTH looped-gradient animation (needs new
// enum strings in the string table) and operator insertion. If it red-errors, we
// need a byte-faithful v5 string-table extender instead.
//
// usage:
//   cargo run -p vpkmerge-core --example reencode_particles -- \
//     <base_dir.vpk> <out_dir.vpk> <prefix>...
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().expect("base_dir.vpk");
    let out = args.next().expect("out_dir.vpk");
    let prefixes: Vec<String> = args.collect();
    assert!(!prefixes.is_empty(), "give at least one path prefix");

    let vpk = valve_pak::open(&base)?;
    let entries: Vec<String> = vpk
        .file_paths()
        .filter(|p| {
            p.ends_with(".vpcf_c") && prefixes.iter().any(|pre| p.starts_with(pre.as_str()))
        })
        .cloned()
        .collect();

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for entry in &entries {
        let original = vpk.get_file(entry)?.read_all()?;
        let result = morphic::decode_kv3_resource(&original)
            .and_then(|value| morphic::encode_kv3_resource(&original, &value));
        match result {
            Ok(reencoded) => packed.push((entry.clone(), reencoded)),
            Err(e) => {
                failed.push(format!("{entry}: {e}"));
            }
        }
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {} of {} particles re-encoded ({} failed to re-encode)",
        packed.len(),
        entries.len(),
        failed.len()
    );
    for f in failed.iter().take(20) {
        println!("  re-encode FAILED: {f}");
    }
    Ok(())
}
