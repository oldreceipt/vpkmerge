// Stability check for the vertex-color recolor: confirm that recoloring a model
// left its GEOMETRY untouched. For each editable (POSITION-bearing) vertex
// buffer, compare positions read from the base pak vs the recolored addon and
// assert they are byte-identical. This is the "a bad model edit crashes / warps
// the game" guard, focused on the meshopt re-encode path (the riskiest).
//
// usage: cargo run --example poscheck -- <base.vpk> <addon.vpk> <entry> [more entries...]
use morphic::model::{read_vertex_positions, vertex_targets};

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let base = valve_pak::open(a.next().expect("base vpk"))?;
    let addon = valve_pak::open(a.next().expect("addon vpk"))?;
    let entries: Vec<String> = a.collect();

    let mut checked = 0usize;
    for entry in &entries {
        let bb = base.get_file(entry).expect("entry in base").read_all()?;
        let ab = addon.get_file(entry).expect("entry in addon").read_all()?;

        // The addon must still parse (a corrupt .vmdl_c would crash on load).
        let targets = vertex_targets(&ab)?;
        for t in &targets {
            // Compare positions on any buffer that exposes a float POSITION
            // (not just the meshopt/`editable` ones), so the hero models'
            // uncompressed buffers are covered too.
            let (Ok(before), Ok(after)) = (
                read_vertex_positions(&bb, t.block_index),
                read_vertex_positions(&ab, t.block_index),
            ) else {
                continue;
            };
            assert_eq!(
                before.len(),
                after.len(),
                "{entry} block {} vertex count changed",
                t.block_index
            );
            let max_d = before
                .iter()
                .zip(&after)
                .flat_map(|(p, q)| (0..3).map(move |k| (p[k] - q[k]).abs()))
                .fold(0.0f32, f32::max);
            let status = if max_d == 0.0 { "IDENTICAL" } else { "DRIFTED" };
            println!(
                "  {entry} block {:>3} ({} verts, meshopt={}): positions {status} (max |d|={max_d})",
                t.block_index, t.vertex_count, t.meshopt
            );
            assert_eq!(max_d, 0.0, "positions drifted: geometry was corrupted");
            checked += 1;
        }
    }
    println!(
        "OK: {checked} editable buffer(s) verified geometry-identical across {} model(s)",
        entries.len()
    );
    Ok(())
}
