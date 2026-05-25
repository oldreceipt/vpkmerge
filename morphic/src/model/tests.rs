//! M3 validation. These tests run in CI off the committed KV3 fixtures, so they
//! cover the parts of model decode that do not need the multi-megabyte vertex
//! buffers: the skeleton (from `DATA`), the LOD0 embedded-mesh registry +
//! vertex layouts (from `CTRL`), and the body mesh's draw calls + scene bounds
//! (from `MDAT[0]`). Everything is diffed against the oracle golden
//! `hornet_model_meta.json` (produced by `morphic-oracle model-meta`, which
//! wraps `ValveResourceFormat`). The full buffer-decode path (positions, joints,
//! the vertex/index totals, the position bbox) is exercised by the gated
//! `tests/model_local.rs` against a real VPK, diffed against the same golden.

use std::path::PathBuf;

use serde::Deserialize;

use super::{mesh, skeleton};
use crate::kv3::{self, Value};

#[derive(Deserialize)]
struct Golden {
    bone_count: usize,
    bone_names: Vec<String>,
    meshes: Vec<GMesh>,
}

#[derive(Deserialize)]
struct GMesh {
    name: String,
    mesh_index: usize,
    scene_min: [f32; 3],
    scene_max: [f32; 3],
    vertex_buffers: Vec<GVb>,
    index_buffers: Vec<GIb>,
    primitives: Vec<GPrim>,
}

#[derive(Deserialize)]
struct GVb {
    element_count: usize,
    element_size: usize,
    fields: Vec<GField>,
}

#[derive(Deserialize)]
struct GField {
    semantic: String,
    semantic_index: i32,
    format: u32,
    offset: usize,
}

#[derive(Deserialize)]
struct GIb {
    element_count: usize,
    element_size: usize,
}

#[derive(Deserialize)]
struct GPrim {
    vertex_buffer: usize,
    vertex_count: usize,
    index_count: usize,
    material: String,
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/kv3")
}

fn parse_fixture(name: &str) -> Value {
    let bytes = std::fs::read(fixtures().join(name)).expect("read kv3 fixture");
    kv3::parse(&bytes).expect("parse kv3 fixture")
}

fn load_golden() -> Golden {
    let text = std::fs::read_to_string(fixtures().join("hornet_model_meta.json"))
        .expect("read model-meta golden");
    serde_json::from_str(&text).expect("parse model-meta golden")
}

fn approx3(a: [f32; 3], b: [f32; 3], what: &str) {
    for i in 0..3 {
        assert!(
            (a[i] - b[i]).abs() < 1e-3,
            "{what}[{i}]: {} vs golden {}",
            a[i],
            b[i]
        );
    }
}

#[test]
fn skeleton_matches_golden() {
    let data = parse_fixture("hornet_data.kv3bin");
    let skel = skeleton::Skeleton::from_model_data(&data).expect("build skeleton");
    let g = load_golden();

    assert_eq!(skel.bones.len(), g.bone_count, "bone count");
    assert_eq!(skel.sorted_bone_names(), g.bone_names, "bone-name set");

    // Hierarchy is well formed: exactly one root, every parent in range and
    // earlier than its child (so global bind poses resolve in one pass).
    let roots = skel.bones.iter().filter(|b| b.parent.is_none()).count();
    assert_eq!(roots, 1, "expected a single root bone");
    for (i, b) in skel.bones.iter().enumerate() {
        if let Some(p) = b.parent {
            assert!(p < i, "bone {i} parent {p} not earlier");
        }
        // Inverse-bind round-trips its global bind to identity (sanity on the
        // matrix math), within float tolerance.
        let prod = b.global_bind.mul(&b.inverse_bind);
        for r in 0..4 {
            for c in 0..4 {
                let expect = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (prod.m[r * 4 + c] - expect).abs() < 1e-3,
                    "bone {i} bind*inverse not identity"
                );
            }
        }
    }
}

#[test]
fn embedded_meshes_and_layouts_match_golden() {
    let data = parse_fixture("hornet_data.kv3bin");
    let ctrl = parse_fixture("hornet_ctrl.kv3bin");
    let embedded = mesh::EmbeddedMesh::parse_all(&ctrl).expect("parse embedded meshes");
    let lod0 = mesh::lod0_indices(&data, &embedded).expect("lod0 filter");
    let g = load_golden();

    assert_eq!(lod0.len(), g.meshes.len(), "LOD0 mesh count");

    for (gm, &idx) in g.meshes.iter().zip(&lod0) {
        let em = &embedded[idx];
        assert_eq!(em.name, gm.name, "mesh name");
        assert_eq!(em.mesh_index, gm.mesh_index, "mesh index");

        assert_eq!(
            em.vertex_buffers.len(),
            gm.vertex_buffers.len(),
            "{}: vertex buffer count",
            gm.name
        );
        for (vb, gvb) in em.vertex_buffers.iter().zip(&gm.vertex_buffers) {
            assert_eq!(vb.element_count, gvb.element_count, "{}: vb count", gm.name);
            assert_eq!(vb.element_size, gvb.element_size, "{}: vb stride", gm.name);
            assert_eq!(
                vb.fields.len(),
                gvb.fields.len(),
                "{}: field count",
                gm.name
            );
            for (f, gf) in vb.fields.iter().zip(&gvb.fields) {
                assert_eq!(f.semantic_name, gf.semantic, "{}: field semantic", gm.name);
                assert_eq!(
                    f.semantic_index, gf.semantic_index,
                    "{}: sem index",
                    gm.name
                );
                assert_eq!(f.format as u32, gf.format, "{}: field format", gm.name);
                assert_eq!(f.offset, gf.offset, "{}: field offset", gm.name);
            }
        }

        assert_eq!(
            em.index_buffers.len(),
            gm.index_buffers.len(),
            "{}: index buffer count",
            gm.name
        );
        for (ib, gib) in em.index_buffers.iter().zip(&gm.index_buffers) {
            assert_eq!(ib.element_count, gib.element_count, "{}: ib count", gm.name);
            assert_eq!(ib.element_size, gib.element_size, "{}: ib width", gm.name);
        }
    }
}

#[test]
fn body_draw_calls_match_golden() {
    // The committed MDAT[0] is the body mesh (golden mesh index 0).
    let mdat = parse_fixture("hornet_mdat0.kv3bin");
    let scene = mesh::SceneObject::parse_all(&mdat).expect("parse scene objects");
    let body = &load_golden().meshes[0];
    assert_eq!(body.name, "body", "golden mesh 0 is body");

    let mut prims = Vec::new();
    let mut smin = [f32::INFINITY; 3];
    let mut smax = [f32::NEG_INFINITY; 3];
    for so in &scene {
        for i in 0..3 {
            smin[i] = smin[i].min(so.min_bounds[i]);
            smax[i] = smax[i].max(so.max_bounds[i]);
        }
        prims.extend(so.draw_calls.iter());
    }

    assert_eq!(prims.len(), body.primitives.len(), "body primitive count");
    for (dc, gp) in prims.iter().zip(&body.primitives) {
        assert_eq!(
            dc.vertex_buffer, gp.vertex_buffer,
            "draw call vertex buffer"
        );
        assert_eq!(dc.vertex_count, gp.vertex_count, "draw call vertex count");
        assert_eq!(dc.index_count, gp.index_count, "draw call index count");
        assert_eq!(dc.material, gp.material, "draw call material");
        assert_eq!(dc.primitive_type, "RENDER_PRIM_TRIANGLES");
    }

    approx3(smin, body.scene_min, "body scene min");
    approx3(smax, body.scene_max, "body scene max");
}

#[test]
fn remap_table_partitions_by_mesh() {
    // Each mesh's blend-index remap slice is non-empty and maps into the model
    // skeleton, so deinterleaved joints stay in range.
    let data = parse_fixture("hornet_data.kv3bin");
    let skel = skeleton::Skeleton::from_model_data(&data).expect("skeleton");

    let body = skeleton::remap_table(&data, 0).expect("body remap table");
    assert!(!body.is_empty(), "body remap table empty");
    assert!(
        body.iter().all(|&b| b < skel.bones.len()),
        "remap maps outside skeleton"
    );
}
