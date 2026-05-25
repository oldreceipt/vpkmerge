//! Local (non-CI) full-model decode check: the end-to-end M3 path that needs
//! the real multi-megabyte vertex buffers, so it is gated on `MORPHIC_MODEL_VPK`
//! pointing at a Deadlock `pak01_dir.vpk` and skipped otherwise. Decodes the
//! hornet hero model (buffers, deinterleave, skin) and diffs the whole result
//! against the committed oracle golden `hornet_model_meta.json`.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct Golden {
    bone_count: usize,
    bone_names: Vec<String>,
    meshes: Vec<GMesh>,
    unique_vertices: usize,
    gltf_vertices: usize,
    total_indices: usize,
    material_count: usize,
    materials: Vec<String>,
    bbox_min: [f32; 3],
    bbox_max: [f32; 3],
}

#[derive(Deserialize)]
struct GMesh {
    name: String,
    mesh_index: usize,
    primitives: Vec<GPrim>,
}

#[derive(Deserialize)]
struct GPrim {
    vertex_buffer: usize,
    vertex_count: usize,
    index_count: usize,
    material: String,
}

fn load_golden() -> Golden {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/kv3/hornet_model_meta.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read golden"))
        .expect("parse golden")
}

fn approx3(a: [f32; 3], b: [f32; 3], what: &str) {
    for i in 0..3 {
        assert!(
            (a[i] - b[i]).abs() < 1e-3,
            "{what}[{i}]: {} vs {}",
            a[i],
            b[i]
        );
    }
}

#[test]
fn decode_hornet_local() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local model decode");
        return;
    };
    let entry = std::env::var("MORPHIC_MODEL_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("locate entry");
    let bytes = vf.read_all().expect("read entry");

    let model = morphic::model::decode(&bytes).expect("decode model");
    let g = load_golden();

    // Skeleton.
    assert_eq!(model.skeleton.bones.len(), g.bone_count, "joint count");
    assert_eq!(
        model.skeleton.sorted_bone_names(),
        g.bone_names,
        "bone names"
    );

    // Totals.
    assert_eq!(model.total_vertices(), g.unique_vertices, "unique vertices");
    assert_eq!(model.gltf_vertex_total(), g.gltf_vertices, "gltf vertices");
    assert_eq!(model.total_indices(), g.total_indices, "total indices");
    assert_eq!(model.materials(), g.materials, "materials");
    assert_eq!(model.materials().len(), g.material_count, "material count");

    // Bounds over decoded positions.
    let bounds = model.position_bounds().expect("position bounds");
    approx3(bounds.min, g.bbox_min, "bbox min");
    approx3(bounds.max, g.bbox_max, "bbox max");

    // Per-mesh primitives + skin attributes.
    assert_eq!(model.meshes.len(), g.meshes.len(), "mesh count");
    for (m, gm) in model.meshes.iter().zip(&g.meshes) {
        assert_eq!(m.name, gm.name, "mesh name");
        assert_eq!(m.mesh_index, gm.mesh_index, "mesh index");
        assert_eq!(
            m.primitives.len(),
            gm.primitives.len(),
            "{}: prim count",
            gm.name
        );

        for (p, gp) in m.primitives.iter().zip(&gm.primitives) {
            assert_eq!(p.vertex_buffer, gp.vertex_buffer, "{}: prim vbuf", gm.name);
            assert_eq!(
                p.vertex_count, gp.vertex_count,
                "{}: prim vert count",
                gm.name
            );
            assert_eq!(
                p.indices.len(),
                gp.index_count,
                "{}: prim index count",
                gm.name
            );
            assert_eq!(p.material, gp.material, "{}: prim material", gm.name);

            // Every index addresses a real vertex in the buffer it draws from.
            let vb = &m.vertex_buffers[p.vertex_buffer];
            assert!(
                p.indices.iter().all(|&i| (i as usize) < vb.element_count),
                "{}: index out of range",
                gm.name
            );
        }

        // Skinned buffers carry one joint set per vertex, positioned and normaled.
        for vb in &m.vertex_buffers {
            assert_eq!(
                vb.positions.len(),
                vb.element_count,
                "{}: positions",
                gm.name
            );
            assert_eq!(vb.normals.len(), vb.element_count, "{}: normals", gm.name);
            if !vb.joints.is_empty() {
                assert_eq!(vb.joints.len(), vb.element_count, "{}: joints", gm.name);
                let max_joint = vb.joints.iter().flatten().copied().max().unwrap_or(0);
                assert!(
                    (max_joint as usize) < model.skeleton.bones.len(),
                    "{}: joint index exceeds skeleton",
                    gm.name
                );
            }
        }
    }

    eprintln!(
        "hornet OK: {} bones, {} meshes, {} unique verts ({} gltf), {} indices, {} materials",
        model.skeleton.bones.len(),
        model.meshes.len(),
        model.total_vertices(),
        model.gltf_vertex_total(),
        model.total_indices(),
        model.materials().len(),
    );

    assert_glb_roundtrip(&model);
}

/// M5a: write the `.glb`, re-read it with the `gltf` crate (which validates the
/// container + accessors), and confirm the structure survived. The file is left
/// at `MORPHIC_GLB_OUT` (default /tmp/hornet.glb) for a maintainer to open in a
/// glTF viewer / Grimoire's three.js and confirm it skins + retargets clips.
fn assert_glb_roundtrip(model: &morphic::model::Model) {
    let glb = morphic::model::to_glb(model).expect("write glb");
    let out = std::env::var("MORPHIC_GLB_OUT").unwrap_or_else(|_| "/tmp/hornet.glb".to_string());
    std::fs::write(&out, &glb).expect("write glb file");
    eprintln!("wrote {} ({} bytes)", out, glb.len());

    let gltf = gltf::Gltf::from_slice(&glb).unwrap_or_else(|e| panic!("re-read glb: {e:?}"));
    assert!(gltf.blob.is_some(), "glb carries its binary blob");
    let doc = &gltf.document;

    let gltf_meshes = doc.meshes().count();
    assert_eq!(gltf_meshes, model.meshes.len(), "glb mesh count");

    let skin = doc.skins().next().expect("glb has a skin");
    assert_eq!(
        skin.joints().count(),
        model.skeleton.bones.len(),
        "glb joint count"
    );

    // Bone names survive (the retarget key).
    let mut glb_bone_names: Vec<String> = skin
        .joints()
        .filter_map(|j| j.name().map(str::to_owned))
        .collect();
    glb_bone_names.sort();
    assert_eq!(
        glb_bone_names,
        model.skeleton.sorted_bone_names(),
        "glb bone names"
    );

    // Every primitive has POSITION + JOINTS_0 and an index accessor.
    let mut prim_total = 0;
    for mesh in doc.meshes() {
        for prim in mesh.primitives() {
            prim_total += 1;
            assert!(
                prim.get(&gltf::Semantic::Positions).is_some(),
                "prim has POSITION"
            );
            assert!(prim.indices().is_some(), "prim has indices");
            assert!(
                prim.get(&gltf::Semantic::Joints(0)).is_some(),
                "skinned prim has JOINTS_0"
            );
        }
    }
    assert_eq!(prim_total, 7, "hornet LOD0 primitive count");
    eprintln!("glb re-read OK: {gltf_meshes} meshes, {prim_total} primitives, valid skin");
}
