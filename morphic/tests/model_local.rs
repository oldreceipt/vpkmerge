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

/// Resolves compiled resource paths across VPKs: the model's own VPK first,
/// then any fallbacks (the base `pak01_dir.vpk` for materials/textures a skin
/// references but does not ship). The caller-side I/O `to_glb_textured` needs.
struct VpkResolver {
    vpks: Vec<valve_pak::VPK>,
}

impl morphic::model::FileResolver for VpkResolver {
    fn resolve(&self, compiled_path: &str) -> Option<Vec<u8>> {
        for vpk in &self.vpks {
            if let Ok(mut vf) = vpk.get_file(compiled_path) {
                if let Ok(bytes) = vf.read_all() {
                    return Some(bytes);
                }
            }
        }
        None
    }
}

/// Diagnostic: per material, report its base-color slot's resolve + decode
/// result (dims, or why it is missing). Gated on `MORPHIC_DIAG_VPK`
/// (+ optional `MORPHIC_DIAG_BASE`, `MORPHIC_DIAG_ENTRY`).
#[test]
fn diagnose_textures() {
    use morphic::model::FileResolver as _;

    let Ok(vpk_path) = std::env::var("MORPHIC_DIAG_VPK") else {
        eprintln!("MORPHIC_DIAG_VPK not set; skipping");
        return;
    };
    let entry = std::env::var("MORPHIC_DIAG_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("entry");
    let bytes = vf.read_all().expect("read");
    let model = morphic::model::decode(&bytes).expect("decode");

    let mut vpks = vec![vpk];
    if let Ok(base) = std::env::var("MORPHIC_DIAG_BASE") {
        vpks.push(valve_pak::open(&base).expect("base"));
    }
    let resolver = VpkResolver { vpks };
    let compiled = |p: &str| {
        if p.ends_with("_c") {
            p.to_string()
        } else {
            format!("{p}_c")
        }
    };

    for mat_path in model.materials() {
        let name = mat_path.rsplit('/').next().unwrap_or(&mat_path).to_string();
        let Some(vmat) = resolver.resolve(&compiled(&mat_path)) else {
            eprintln!("MAT {name}: vmat UNRESOLVED");
            continue;
        };
        let mat = morphic::material::parse(&vmat).expect("parse vmat");
        let Some(base) = mat.pbr().base_color else {
            eprintln!("MAT {name} [{}]: no base_color slot", mat.shader_name);
            continue;
        };
        let status = match resolver.resolve(&compiled(base)) {
            None => "UNRESOLVED".to_string(),
            Some(b) => match morphic::decode(&b) {
                Err(e) => format!("decode-err: {e}"),
                Ok(img) => format!("{}x{}", img.width, img.height),
            },
        };
        eprintln!(
            "MAT {name} [{}]: base_color {} -> {status}",
            mat.shader_name,
            base.rsplit('/').next().unwrap_or(base)
        );
    }
}

/// Gated textured GLB export of an arbitrary VPK entry (no golden diff): set
/// `MORPHIC_EXPORT_VPK` (+ optional `MORPHIC_EXPORT_ENTRY`, `MORPHIC_EXPORT_OUT`,
/// and `MORPHIC_EXPORT_BASE` for the base pak that skins reference). Decodes the
/// model and writes a textured `.glb`. A stand-in for the M6 `model export` CLI.
#[test]
fn export_glb_from_env() {
    let Ok(vpk_path) = std::env::var("MORPHIC_EXPORT_VPK") else {
        eprintln!("MORPHIC_EXPORT_VPK not set; skipping arbitrary export");
        return;
    };
    let entry = std::env::var("MORPHIC_EXPORT_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());
    let out = std::env::var("MORPHIC_EXPORT_OUT").unwrap_or_else(|_| "/tmp/export.glb".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("locate entry");
    let bytes = vf.read_all().expect("read entry");
    let model = morphic::model::decode(&bytes).expect("decode model");

    // Resolver: the model's VPK first, then the base pak (if given).
    let mut vpks = vec![vpk];
    if let Ok(base) = std::env::var("MORPHIC_EXPORT_BASE") {
        vpks.push(valve_pak::open(&base).expect("open base pak"));
    }
    let resolver = VpkResolver { vpks };

    let glb = morphic::model::to_glb_textured(&model, &resolver).expect("write glb");
    std::fs::write(&out, &glb).expect("write glb file");
    eprintln!(
        "exported {entry} -> {out} ({} bytes): {} bones, {} meshes, {} unique verts, {} materials",
        glb.len(),
        model.skeleton.bones.len(),
        model.meshes.len(),
        model.total_vertices(),
        model.materials().len(),
    );
}

/// Lists hero body-model entries in a VPK: `<dir>/<name>/<name>.vmdl_c` under a
/// `models/heroes*` path (the body model convention, skipping LODs/backups/props).
/// Set `MORPHIC_DIAG_VPK`; skipped otherwise. Cheap (path listing only, no decode).
#[test]
fn list_heroes() {
    let Ok(vpk_path) = std::env::var("MORPHIC_DIAG_VPK") else {
        eprintln!("MORPHIC_DIAG_VPK not set; skipping");
        return;
    };
    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut hits: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vmdl_c") && p.contains("/heroes"))
        .filter(|p| {
            let stem = p
                .rsplit('/')
                .next()
                .unwrap_or(p)
                .trim_end_matches(".vmdl_c");
            let parent = p.rsplit('/').nth(1).unwrap_or("");
            stem == parent
        })
        .cloned()
        .collect();
    hits.sort();
    hits.dedup();
    for h in &hits {
        eprintln!("HERO {h}");
    }
    eprintln!("HEROCOUNT {}", hits.len());
}

/// Lists EVERY `.vmdl_c` under `/heroes` (minus obvious non-body parts), for
/// finding a hero's real/current body model (which may be in a `_vN` dir, e.g.
/// `hornet_v3/hornet.vmdl_c`). Set `MORPHIC_DIAG_VPK`; optional `MORPHIC_DIAG_GREP`
/// filters by substring. Cheap (path listing only).
#[test]
fn list_hero_vmdl_all() {
    let Ok(vpk_path) = std::env::var("MORPHIC_DIAG_VPK") else {
        eprintln!("MORPHIC_DIAG_VPK not set; skipping");
        return;
    };
    let needle = std::env::var("MORPHIC_DIAG_GREP")
        .unwrap_or_default()
        .to_lowercase();
    let skip = [
        "_lod",
        "lod0",
        "lod1",
        "lod2",
        "lod3",
        "backup",
        "/clips/",
        "abilities",
        "particle",
        "/materials/",
        "_dbg",
        "destruction",
    ];
    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut hits: Vec<String> = vpk
        .file_paths()
        .filter(|p| p.ends_with(".vmdl_c") && p.contains("/heroes"))
        .filter(|p| {
            let lc = p.to_lowercase();
            !skip.iter().any(|s| lc.contains(s)) && (needle.is_empty() || lc.contains(&needle))
        })
        .cloned()
        .collect();
    hits.sort();
    hits.dedup();
    for h in &hits {
        eprintln!("VMDL {h}");
    }
    eprintln!("VMDLCOUNT {}", hits.len());
}

/// Lists the animation clips a model carries. Set `MORPHIC_DIAG_VPK`
/// (+ optional `MORPHIC_DIAG_ENTRY`); skipped otherwise. Diagnostic for picking a
/// `--pose` clip.
#[test]
fn list_clips() {
    let Ok(vpk_path) = std::env::var("MORPHIC_DIAG_VPK") else {
        eprintln!("MORPHIC_DIAG_VPK not set; skipping");
        return;
    };
    let entry = std::env::var("MORPHIC_DIAG_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());
    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("entry");
    let bytes = vf.read_all().expect("read");
    let model = morphic::model::decode(&bytes).expect("decode");
    eprintln!("CLIPS {} ({} total):", entry, model.animations.len());
    for c in &model.animations {
        eprintln!("  {} ({} frames)", c.name, c.frame_count);
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

/// Tier-0 displacement edit, end to end on the real hornet model: enumerate
/// vertex targets, translate one buffer's positions, splice it back, and confirm
/// the edited `.vmdl_c` (a) re-decodes, (b) shows exactly that buffer's positions
/// shifted by the delta, and (c) leaves every other attribute and buffer
/// byte-identical. Gated on `MORPHIC_MODEL_VPK`; run with the local Deadlock pak.
#[test]
fn displacement_edit_round_trips_local() {
    let Ok(vpk_path) = std::env::var("MORPHIC_MODEL_VPK") else {
        eprintln!("MORPHIC_MODEL_VPK not set; skipping local displacement edit");
        return;
    };
    let entry = std::env::var("MORPHIC_MODEL_ENTRY")
        .unwrap_or_else(|_| "models/heroes_staging/hornet_v3/hornet.vmdl_c".to_string());

    let vpk = valve_pak::open(&vpk_path).expect("open vpk");
    let mut vf = vpk.get_file(&entry).expect("locate entry");
    let bytes = vf.read_all().expect("read entry");

    // Pick the first displacement-editable vertex buffer.
    let targets = morphic::model::vertex_targets(&bytes).expect("targets");
    let target = targets
        .iter()
        .find(|t| t.editable)
        .expect("at least one editable vertex buffer");
    eprintln!(
        "editing buffer: mesh={} block={} verts={} stride={}",
        target.mesh_name, target.block_index, target.vertex_count, target.stride
    );

    // Translate the whole buffer by a fixed delta.
    let delta = [5.0f32, -3.0, 2.0];
    let orig_pos = morphic::model::read_vertex_positions(&bytes, target.block_index).expect("read");
    assert_eq!(orig_pos.len(), target.vertex_count, "position count");
    let moved: Vec<[f32; 3]> = orig_pos
        .iter()
        .map(|p| [p[0] + delta[0], p[1] + delta[1], p[2] + delta[2]])
        .collect();

    let edited = morphic::model::replace_vertex_positions(&bytes, target.block_index, &moved)
        .expect("splice");

    // (a) The edited buffer's positions read back shifted by exactly the delta.
    let new_pos =
        morphic::model::read_vertex_positions(&edited, target.block_index).expect("read edited");
    assert_eq!(new_pos.len(), orig_pos.len(), "edited position count");
    for (o, n) in orig_pos.iter().zip(&new_pos) {
        for k in 0..3 {
            assert!(
                (n[k] - (o[k] + delta[k])).abs() <= 1e-3,
                "position not shifted by delta: {o:?} -> {n:?}"
            );
        }
    }

    // (b)+(c) Full re-decode: same structure; only the edited buffer's positions
    // changed, everything else (normals/uv/joints/weights/indices) byte-identical.
    let before = morphic::model::decode(&bytes).expect("decode original");
    let after = morphic::model::decode(&edited).expect("decode edited");
    assert_eq!(
        before.meshes.len(),
        after.meshes.len(),
        "mesh count preserved"
    );
    assert_eq!(
        before.total_indices(),
        after.total_indices(),
        "index total preserved"
    );

    let mut changed_buffers = 0usize;
    for (mb, ma) in before.meshes.iter().zip(&after.meshes) {
        assert_eq!(mb.name, ma.name, "mesh name");
        assert_eq!(
            mb.vertex_buffers.len(),
            ma.vertex_buffers.len(),
            "buffer count"
        );
        for (vb, va) in mb.vertex_buffers.iter().zip(&ma.vertex_buffers) {
            // Non-position attributes are always preserved.
            assert_eq!(vb.normals, va.normals, "{}: normals", mb.name);
            assert_eq!(vb.tangents, va.tangents, "{}: tangents", mb.name);
            assert_eq!(vb.texcoords, va.texcoords, "{}: texcoords", mb.name);
            assert_eq!(vb.joints, va.joints, "{}: joints", mb.name);
            assert_eq!(vb.weights, va.weights, "{}: weights", mb.name);
            if vb.positions != va.positions {
                changed_buffers += 1;
            }
        }
        // Primitive indices are unchanged everywhere.
        for (pb, pa) in mb.primitives.iter().zip(&ma.primitives) {
            assert_eq!(pb.indices, pa.indices, "{}: indices", mb.name);
        }
    }
    assert_eq!(
        changed_buffers, 1,
        "exactly one vertex buffer's positions changed"
    );
    eprintln!("displacement edit OK: 1 buffer moved, all other attributes preserved");
}

/// Mirrors the GLB writer's shell rule (inverted-hull `*_outline` and additive
/// `*_glow`, but not `*_noglow`): such geometry is dropped from the export.
fn is_shell(s: &str) -> bool {
    let lc = s.to_ascii_lowercase();
    lc.contains("outline") || (lc.contains("glow") && !lc.contains("noglow"))
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

    // The GLB writer drops parts that are entirely non-renderable NPR shells
    // (inverted-hull `*_outline` and additive `*_glow`), so the glTF carries the
    // model's renderable parts, not every part. Mirror that rule and check the
    // surviving meshes are exactly those parts (by name).
    let mut expected_meshes: Vec<&str> = model
        .meshes
        .iter()
        .filter(|p| !is_shell(&p.name) && p.primitives.iter().any(|pr| !is_shell(&pr.material)))
        .map(|p| p.name.as_str())
        .collect();
    expected_meshes.sort_unstable();
    let mut glb_mesh_names: Vec<&str> = doc.meshes().filter_map(|m| m.name()).collect();
    glb_mesh_names.sort_unstable();
    assert_eq!(
        glb_mesh_names, expected_meshes,
        "glb keeps exactly the renderable (non-shell) parts"
    );

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
    // body (5) + gun (1); the `ghost_glow` shell part (1 prim) is dropped.
    assert_eq!(prim_total, 6, "hornet LOD0 primitive count");
    eprintln!(
        "glb re-read OK: {} meshes, {prim_total} primitives, valid skin",
        glb_mesh_names.len()
    );
}
