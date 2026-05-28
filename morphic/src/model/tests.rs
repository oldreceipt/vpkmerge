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
    kv3::decode(&bytes).expect("parse kv3 fixture")
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

// --- GLB writer (M5a) ---

use super::math::{Mat4, Quat, Vec3};
use super::mesh::{MeshPart, Primitive, VertexBuffer};
use super::skeleton::Bone;
use super::{BoneTrack, Clip, Model};

/// A minimal skinned model: one root bone, one triangle bound to it. Lets the
/// GLB writer be exercised in CI without the multi-megabyte hornet buffers.
fn synthetic_model() -> Model {
    let bone = Bone {
        name: "root".to_owned(),
        parent: None,
        flags: 0,
        position: Vec3::default(),
        rotation: Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        local_bind: Mat4::IDENTITY,
        global_bind: Mat4::IDENTITY,
        inverse_bind: Mat4::IDENTITY,
    };
    let vb = VertexBuffer {
        element_count: 3,
        stride: 0,
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        tangents: Vec::new(),
        texcoords: vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]],
        joints: vec![[0, 0, 0, 0]; 3],
        weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
        layout: Vec::new(),
    };
    let part = MeshPart {
        name: "tri".to_owned(),
        mesh_index: 0,
        vertex_buffers: vec![vb],
        primitives: vec![Primitive {
            vertex_buffer: 0,
            material: "test/mat.vmat".to_owned(),
            vertex_count: 3,
            indices: vec![0, 1, 2],
        }],
        min_bounds: [0.0; 3],
        max_bounds: [1.0, 1.0, 0.0],
        bone_weight_count: 1,
    };
    Model {
        skeleton: skeleton::Skeleton { bones: vec![bone] },
        meshes: vec![part],
        animations: Vec::new(),
    }
}

#[test]
fn glb_writes_and_reloads() {
    let glb = super::to_glb(&synthetic_model()).expect("write glb");

    // Valid container: "glTF" magic, version 2, length matches.
    assert_eq!(&glb[0..4], b"glTF", "GLB magic");
    assert_eq!(
        u32::from_le_bytes(glb[4..8].try_into().unwrap()),
        2,
        "GLB version"
    );
    assert_eq!(
        u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
        glb.len(),
        "GLB declared length"
    );

    // The `gltf` reader parses + validates it.
    let g = gltf::Gltf::from_slice(&glb).expect("re-read glb");
    let doc = &g.document;

    assert_eq!(doc.meshes().count(), 1);
    let prim_count: usize = doc.meshes().map(|m| m.primitives().count()).sum();
    assert_eq!(prim_count, 1);

    let skin = doc.skins().next().expect("has skin");
    assert_eq!(skin.joints().count(), 1, "one joint");
    assert_eq!(
        skin.joints().next().unwrap().name(),
        Some("root"),
        "bone name preserved"
    );

    let prim = doc.meshes().next().unwrap().primitives().next().unwrap();
    assert!(prim.get(&gltf::Semantic::Positions).is_some(), "POSITION");
    assert!(prim.get(&gltf::Semantic::Joints(0)).is_some(), "JOINTS_0");
    assert!(prim.get(&gltf::Semantic::Weights(0)).is_some(), "WEIGHTS_0");
    assert!(prim.indices().is_some(), "indices");
}

/// [`synthetic_model`] with a single 2-frame rotation clip on its one bone:
/// exercises the animation emit path (samplers/channels/accessors) in CI with no
/// VPK dependency.
fn synthetic_animated_model() -> Model {
    let mut m = synthetic_model();
    m.animations = vec![Clip {
        name: "spin".to_owned(),
        fps: 2.0,
        frame_count: 2,
        looping: true,
        tracks: vec![BoneTrack {
            bone: 0,
            translations: None,
            rotations: Some(vec![
                Quat {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                },
                Quat {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                    w: 0.0,
                },
            ]),
            scales: None,
        }],
    }];
    m
}

#[test]
fn glb_animation_writes_and_reloads() {
    let glb = super::to_glb(&synthetic_animated_model()).expect("write glb");
    let g = gltf::Gltf::from_slice(&glb).expect("re-read glb");
    let doc = &g.document;

    assert_eq!(doc.animations().count(), 1, "one clip");
    let anim = doc.animations().next().unwrap();
    assert_eq!(anim.name(), Some("spin"));
    assert_eq!(anim.channels().count(), 1, "one rotation channel");
    assert_eq!(anim.samplers().count(), 1, "one sampler");

    let chan = anim.channels().next().unwrap();
    assert!(
        matches!(
            chan.target().property(),
            gltf::animation::Property::Rotation
        ),
        "targets rotation"
    );
    assert_eq!(
        chan.target().node().name(),
        Some("root"),
        "targets the joint node"
    );

    let s = chan.sampler();
    assert!(
        matches!(s.interpolation(), gltf::animation::Interpolation::Linear),
        "linear interpolation"
    );
    assert_eq!(s.input().count(), 2, "two time samples");
    assert_eq!(s.output().count(), 2, "two rotation samples");
    assert_eq!(s.input().dimensions(), gltf::accessor::Dimensions::Scalar);
    assert!(s.input().min().is_some(), "input accessor has min");
    assert!(s.input().max().is_some(), "input accessor has max");
    assert_eq!(s.output().dimensions(), gltf::accessor::Dimensions::Vec4);
}

/// A [`super::FileResolver`] backed by committed fixtures: any `.vmat_c` request
/// returns the hornet head material, any `.vtex_c` returns a real BC7 texture.
struct FixtureResolver {
    vmat: Vec<u8>,
    vtex: Vec<u8>,
}

impl super::FileResolver for FixtureResolver {
    fn resolve(&self, compiled_path: &str) -> Option<Vec<u8>> {
        if compiled_path.ends_with(".vmat_c") {
            Some(self.vmat.clone())
        } else if compiled_path.ends_with(".vtex_c") {
            Some(self.vtex.clone())
        } else {
            None
        }
    }
}

#[test]
fn glb_textured_embeds_resolved_images() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let resolver = FixtureResolver {
        vmat: std::fs::read(root.join("fixtures/material/vindicta_headv2.vmat_c"))
            .expect("vmat fixture"),
        // 128x64 BC7, comfortably above the 4x4 placeholder cutoff.
        vtex: std::fs::read(root.join("fixtures/bc7/generic_sleep_icon.vtex_c"))
            .expect("vtex fixture"),
    };

    let glb = super::to_glb_textured(&synthetic_model(), &resolver).expect("textured glb");
    let g = gltf::Gltf::from_slice(&glb).expect("re-read glb");
    let doc = &g.document;

    assert!(doc.images().count() > 0, "embedded images present");
    assert!(doc.textures().count() > 0, "textures present");
    let mat = doc.materials().next().expect("a material");
    assert!(
        mat.pbr_metallic_roughness().base_color_texture().is_some(),
        "base color texture wired"
    );
    // pbr.vfx exposes g_tNormalRoughness, so a normal map is wired too.
    assert!(mat.normal_texture().is_some(), "normal texture wired");
}

#[test]
fn outline_materials_are_detected() {
    assert!(super::glb::is_outline_material(
        "models/heroes_staging/hornet_v3/materials/vindicta_outline.vmat"
    ));
    assert!(!super::glb::is_outline_material(
        "models/heroes_staging/hornet_v3/materials/skinmaterial.vmat"
    ));
}

#[test]
fn glow_shells_are_detected_but_noglow_is_kept() {
    // The additive glow effect shell (mesh part `ghost_glow`, material
    // `*_glow.vmat`) is a shell to drop.
    assert!(super::glb::is_glow_material("ghost_glow"));
    assert!(super::glb::is_glow_material(
        "models/heroes_staging/hornet_v3/materials/vindicta_glow.vmat"
    ));
    assert!(super::glb::is_shell("ghost_glow"));
    // A normal material that merely has glow turned off must be kept.
    assert!(!super::glb::is_glow_material(
        "models/heroes_staging/astro/materials/astro_barrelv2_noglow.vmat"
    ));
    assert!(!super::glb::is_shell(
        "models/heroes_staging/astro/materials/astro_barrelv2_noglow.vmat"
    ));
    assert!(!super::glb::is_shell("body"));
}

mod pose_bake {
    use super::super::animation::{BoneTrack, Clip};
    use super::super::math::{Mat4, Quat, Vec3};
    use super::super::mesh::{MeshPart, Primitive, VertexBuffer};
    use super::super::skeleton::{Bone, Skeleton};
    use super::super::{bake_pose, bake_pose_from, Model};

    const ID: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    fn root_bone() -> Bone {
        Bone {
            name: "root".into(),
            parent: None,
            flags: 0,
            position: Vec3::default(),
            rotation: ID,
            local_bind: Mat4::IDENTITY,
            global_bind: Mat4::IDENTITY,
            inverse_bind: Mat4::IDENTITY,
        }
    }

    fn skinned_vertex(pos: [f32; 3]) -> VertexBuffer {
        VertexBuffer {
            element_count: 1,
            stride: 0,
            positions: vec![pos],
            normals: vec![[0.0, 1.0, 0.0]],
            tangents: vec![],
            texcoords: vec![],
            joints: vec![[0, 0, 0, 0]],
            weights: vec![[1.0, 0.0, 0.0, 0.0]],
            layout: vec![],
        }
    }

    fn one_part(vb: VertexBuffer) -> MeshPart {
        MeshPart {
            name: "body".into(),
            mesh_index: 0,
            primitives: vec![Primitive {
                vertex_buffer: 0,
                material: "body".into(),
                vertex_count: vb.element_count,
                indices: vec![0],
            }],
            vertex_buffers: vec![vb],
            min_bounds: [0.0; 3],
            max_bounds: [0.0; 3],
            bone_weight_count: 1,
        }
    }

    fn single_bone_clip(name: &str, t: Vec3) -> Clip {
        Clip {
            name: name.into(),
            fps: 30.0,
            frame_count: 1,
            looping: false,
            tracks: vec![BoneTrack {
                bone: 0,
                translations: Some(vec![t]),
                rotations: Some(vec![ID]),
                scales: None,
            }],
        }
    }

    fn approx(a: [f32; 3], b: [f32; 3]) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-4, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn bind_pose_clip_leaves_vertices_unchanged() {
        let model = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![one_part(skinned_vertex([1.0, 2.0, 3.0]))],
            animations: vec![single_bone_clip("ui_hero_pose", Vec3::default())],
        };
        let baked = bake_pose(&model, &["ui_hero_pose"], 0);
        assert!(baked.skeleton.bones.is_empty(), "skeleton stripped");
        assert!(baked.animations.is_empty(), "animations stripped");
        let vb = &baked.meshes[0].vertex_buffers[0];
        assert!(vb.joints.is_empty() && vb.weights.is_empty(), "skin stripped");
        approx(vb.positions[0], [1.0, 2.0, 3.0]);
    }

    #[test]
    fn translation_pose_shifts_vertices() {
        let model = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![one_part(skinned_vertex([1.0, 2.0, 3.0]))],
            animations: vec![single_bone_clip(
                "ui_hero_pose",
                Vec3 {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            )],
        };
        let baked = bake_pose(&model, &["ui_hero_pose"], 0);
        approx(
            baked.meshes[0].vertex_buffers[0].positions[0],
            [11.0, 2.0, 3.0],
        );
    }

    #[test]
    fn no_matching_clip_falls_back_to_static_bind() {
        let model = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![one_part(skinned_vertex([4.0, 5.0, 6.0]))],
            animations: vec![single_bone_clip(
                "walk",
                Vec3 {
                    x: 99.0,
                    y: 0.0,
                    z: 0.0,
                },
            )],
        };
        let baked = bake_pose(&model, &["ui_hero_pose"], 0);
        assert!(baked.skeleton.bones.is_empty());
        let vb = &baked.meshes[0].vertex_buffers[0];
        assert!(vb.joints.is_empty() && vb.weights.is_empty());
        approx(vb.positions[0], [4.0, 5.0, 6.0]);
    }

    #[test]
    fn prop_without_skeleton_passes_through() {
        let model = Model {
            skeleton: Skeleton { bones: vec![] },
            meshes: vec![one_part(skinned_vertex([5.0, 6.0, 7.0]))],
            animations: vec![],
        };
        let baked = bake_pose(&model, &["ui_hero_pose"], 0);
        let vb = &baked.meshes[0].vertex_buffers[0];
        assert!(vb.joints.is_empty() && vb.weights.is_empty());
        approx(vb.positions[0], [5.0, 6.0, 7.0]);
    }

    #[test]
    fn donor_clip_poses_a_clipless_skin_by_bone_name() {
        // The skin ships the mesh + rig but no clips (like a real skin mod).
        let skin = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![one_part(skinned_vertex([1.0, 2.0, 3.0]))],
            animations: vec![],
        };
        // The base hero supplies the clip; same bone name "root".
        let donor = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![],
            animations: vec![single_bone_clip(
                "ui_hero_pose",
                Vec3 {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            )],
        };
        let baked = bake_pose_from(&skin, &donor, &["ui_hero_pose"], 0);
        approx(
            baked.meshes[0].vertex_buffers[0].positions[0],
            [11.0, 2.0, 3.0],
        );
    }

    #[test]
    fn donor_bone_name_mismatch_keeps_bind_pose() {
        let skin = Model {
            skeleton: Skeleton {
                bones: vec![root_bone()],
            },
            meshes: vec![one_part(skinned_vertex([1.0, 2.0, 3.0]))],
            animations: vec![],
        };
        let mut donor_bone = root_bone();
        donor_bone.name = "unrelated".into();
        let donor = Model {
            skeleton: Skeleton {
                bones: vec![donor_bone],
            },
            meshes: vec![],
            animations: vec![single_bone_clip(
                "ui_hero_pose",
                Vec3 {
                    x: 10.0,
                    y: 0.0,
                    z: 0.0,
                },
            )],
        };
        // The donor's clip targets "unrelated"; the skin has "root", so no bone
        // matches and the vertex stays at its bind position.
        let baked = bake_pose_from(&skin, &donor, &["ui_hero_pose"], 0);
        approx(
            baked.meshes[0].vertex_buffers[0].positions[0],
            [1.0, 2.0, 3.0],
        );
    }
}
