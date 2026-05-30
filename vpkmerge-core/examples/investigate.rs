// Byte-level investigation of the vertex-color recolor encode path.
// usage: cargo run --example investigate -- <vpk> <entry>
use morphic::model::{decode, read_vertex_colors, vertex_targets};

fn main() -> anyhow::Result<()> {
    let mut a = std::env::args().skip(1);
    let vpk = valve_pak::open(&a.next().expect("vpk"))?;
    let entry = a.next().expect("entry");
    let orig = vpk.get_file(&entry).expect("entry").read_all()?;

    let target = vertex_targets(&orig)?
        .into_iter()
        .find(|t| t.has_color)
        .expect("color buffer");
    println!("entry: {entry}");
    println!(
        "target: mesh={} block={} verts={} meshopt={} stride={}",
        target.mesh_name, target.block_index, target.vertex_count, target.meshopt, target.stride
    );

    // IDENTITY recolor.
    let (ident, lanes) = morphic::model::recolor_vertex_buffer(&orig, target.block_index, |c| c)?;
    println!("\n=== IDENTITY recolor ({lanes} lane) ===");
    println!(
        "orig file: {} bytes   ident file: {} bytes   ({})",
        orig.len(),
        ident.len(),
        if orig.len() == ident.len() {
            "same size"
        } else {
            "SIZE CHANGED"
        }
    );

    // Full-file byte diff.
    let minlen = orig.len().min(ident.len());
    let mut diffs = Vec::new();
    for i in 0..minlen {
        if orig[i] != ident[i] {
            diffs.push(i);
            if diffs.len() >= 12 {
                break;
            }
        }
    }
    if orig == ident {
        println!("IDENTITY IS BYTE-IDENTICAL to original  (rebuild is faithful)");
    } else {
        println!(
            "IDENTITY DIFFERS from original. first diff offsets: {:?}",
            diffs
        );
        // classify first diff
        let d = diffs[0];
        let region = if d < 16 { "HEADER" } else { "table-or-payload" };
        println!("  first diff at {d} (0x{d:x}) in {region}");
        println!(
            "  orig[{}..{}]  = {:02x?}",
            d,
            (d + 16).min(orig.len()),
            &orig[d..(d + 16).min(orig.len())]
        );
        println!(
            "  ident[{}..{}] = {:02x?}",
            d,
            (d + 16).min(ident.len()),
            &ident[d..(d + 16).min(ident.len())]
        );
        // header bytes side by side
        println!("  orig header : {:02x?}", &orig[..16.min(orig.len())]);
        println!("  ident header: {:02x?}", &ident[..16.min(ident.len())]);
    }

    // Attribute integrity: identity must preserve EVERYTHING (incl. colors).
    println!("\n=== attribute integrity (identity: all must match) ===");
    let mo = decode(&orig)?;
    let mi = decode(&ident)?;
    let mut bad = 0;
    for (mb, ma) in mo.meshes.iter().zip(&mi.meshes) {
        for (vb, va) in mb.vertex_buffers.iter().zip(&ma.vertex_buffers) {
            if vb.positions != va.positions {
                println!("  {} positions DIFFER", mb.name);
                bad += 1;
            }
            if vb.normals != va.normals {
                println!("  {} normals DIFFER", mb.name);
                bad += 1;
            }
            if vb.tangents != va.tangents {
                println!("  {} tangents DIFFER", mb.name);
                bad += 1;
            }
            if vb.texcoords != va.texcoords {
                println!("  {} texcoords DIFFER", mb.name);
                bad += 1;
            }
            if vb.joints != va.joints {
                println!("  {} joints DIFFER", mb.name);
                bad += 1;
            }
            if vb.weights != va.weights {
                println!("  {} weights DIFFER", mb.name);
                bad += 1;
            }
        }
    }
    let co = read_vertex_colors(&orig, target.block_index)?.unwrap();
    let ci = read_vertex_colors(&ident, target.block_index)?.unwrap();
    if co != ci {
        println!("  COLORS DIFFER under identity (BUG)");
        bad += 1;
    }
    if bad == 0 {
        println!("  all attributes byte-identical under identity  OK");
    }
    Ok(())
}
