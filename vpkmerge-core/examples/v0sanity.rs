// Sanity-check morphic's meshopt v0 vertex decode: decode a model, print position
// bounds + sample positions/normals. Garbage bounds => v0 decode is the bug.
fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(&a.next().unwrap())?;
    let e = a.next().unwrap();
    let b = vpk.get_file(&e).unwrap().read_all()?;
    let m = morphic::model::decode(&b)?;
    println!(
        "{e}: {} meshes, {} total verts",
        m.meshes.len(),
        m.total_vertices()
    );
    if let Some(bb) = m.position_bounds() {
        println!("  bounds min={:?} max={:?}", bb.min, bb.max);
        let span = [
            bb.max[0] - bb.min[0],
            bb.max[1] - bb.min[1],
            bb.max[2] - bb.min[2],
        ];
        println!(
            "  span={:?}  (a horse/knight should be tens of source units, finite, not 0/NaN/huge)",
            span
        );
    }
    for mesh in &m.meshes {
        for vb in &mesh.vertex_buffers {
            let p = &vb.positions;
            let n = &vb.normals;
            println!(
                "  mesh {} vb: {} verts  pos[0..2]={:?}  normal[0]={:?}",
                mesh.name,
                p.len(),
                &p[..p.len().min(2)],
                n.first()
            );
            let bad = p
                .iter()
                .filter(|q| q.iter().any(|c| !c.is_finite()))
                .count();
            if bad > 0 {
                println!("    !! {bad} non-finite positions (v0 decode broken)");
            }
            break;
        }
    }
    Ok(())
}
