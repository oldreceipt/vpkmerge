// Animate a prism-recolored Yamato VPK:
//   1. make existing spectrum color gradients CYCLE by flipping each age-driven
//      gradient driver to looped collection-age;
//   2. append a runtime color-cycle operator to static m_ConstantColor particles
//      that had no age-driven gradient to loop.
//
// Both paths are byte-faithful KV3 structural edits (string-table append and
// array-element insert), so the compiled particles stay engine-loadable. Re-packs
// EVERY entry, preserving non-particle overrides such as recolored `.vtex_c`
// files, so the output is a complete drop-in replacement for the prism VPK.
//
// usage:
//   cargo run -p vpkmerge-core --example yamato_loop_anim -- \
//     <yamato_prism_dir.vpk> <out_loop_dir.vpk>
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("yamato_prism_dir.vpk");
    let out = args.next().expect("out_loop_dir.vpk");

    let vpk = valve_pak::open(&input)?;
    let mut entries: Vec<String> = vpk.file_paths().cloned().collect();
    entries.sort();

    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    let mut particles = 0usize;
    let mut looped = 0usize;
    let mut inserted = 0usize;
    let mut skipped_err = 0usize;
    for entry in &entries {
        let bytes = vpk.get_file(entry)?.read_all()?;
        if !entry.ends_with(".vpcf_c") {
            packed.push((entry.clone(), bytes));
            continue;
        }

        particles += 1;
        let mut working = bytes;
        match vpkmerge_core::loop_animate_particle_bytes(&working) {
            Ok(Some(new_bytes)) => {
                working = new_bytes;
                looped += 1;
            }
            // No loopable gradient: try the static-color operator insertion below.
            Ok(None) => {}
            // A particle the string-add patch could not touch (e.g. a v4 block missing
            // the enum string): leave it prism-colored but unlooped rather than abort.
            Err(e) => {
                skipped_err += 1;
                eprintln!("  note: not looping {entry} (left as prism): {e:#}");
            }
        }

        match vpkmerge_core::insert_color_cycle_operator(&working) {
            Ok(Some(new_bytes)) => {
                working = new_bytes;
                inserted += 1;
            }
            Ok(None) => {}
            Err(e) => {
                skipped_err += 1;
                eprintln!("  note: not inserting color cycle for {entry} (left as prism): {e:#}");
            }
        }

        packed.push((entry.clone(), working));
    }

    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, &out)?;
    println!(
        "wrote {out}: {} entries, {particles} particles, {looped} gradients looped, {inserted} color-cycle operators inserted, {skipped_err} animation edits skipped",
        refs.len(),
    );
    Ok(())
}
