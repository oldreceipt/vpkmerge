//! Binary glTF (`.glb`) writer for a decoded [`Model`]. Builds the glTF 2.0
//! document with `gltf-json` and frames the GLB container by hand.
//!
//! This is the M5a slice: geometry + skeleton + skin + *untextured* materials
//! (a named default-PBR material per draw-call material path). Texture
//! resolution/decode/embedding is M5b/M6 (needs the cross-VPK loader).
//!
//! Coordinate handling mirrors VRF `GltfModelExporter`: vertex positions and
//! bone local transforms stay in Source space; a wrapper node over the skeleton
//! and each mesh carries `TRANSFORMSOURCETOGLTF` (inches->meters + Z-up->Y-up),
//! and the inverse-bind matrices are the Source-space `inverse(globalBind)`
//! (computed before that transform, per VRF's "order matters"). Keeping bone
//! local transforms in Source space is what lets Grimoire retarget the shared
//! clips by bone name.
//!
//! morphic's [`Mat4`] is row-major / row-vector; glTF stores column-major /
//! column-vector. Row-major storage of `M` equals column-major storage of
//! `Mᵀ`, and `Mᵀ` is exactly the column-vector form of a row-vector `M`, so a
//! `Mat4.m` array is emitted into glTF verbatim.

// Matrices/positions widen between f32 and JSON f64 (serde) losslessly enough;
// buffer lengths are small. These casts are deliberate. `default_trait_access`
// is allowed because gltf-json structs carry many `extras`/`extensions` fields
// that are idiomatically initialized with `Default::default()`.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::default_trait_access
)]

use std::collections::BTreeMap;

use gltf_json as json;
use json::validation::Checked::Valid;
use json::validation::USize64;

use crate::error::DecodeError;

use super::math::{Mat4, Quat};
use super::mesh::{MeshPart, VertexBuffer};
use super::{Model, Skeleton};

/// Source space (inches, Z-up) to glTF space (meters, Y-up). Matches VRF
/// `TRANSFORMSOURCETOGLTF = CreateScale(0.0254) * CreateFromYawPitchRoll(0, -pi/2, -pi/2)`.
fn transform_source_to_gltf() -> Mat4 {
    use std::f32::consts::FRAC_PI_2;
    let rot = Mat4::from_quaternion(Quat::from_yaw_pitch_roll(0.0, -FRAC_PI_2, -FRAC_PI_2));
    Mat4::from_scale(0.0254).mul(&rot)
}

/// Writes `model` as a binary glTF (`.glb`) byte stream.
pub fn to_glb(model: &Model) -> Result<Vec<u8>, DecodeError> {
    let mut b = Builder::default();
    b.root.asset.generator = Some("morphic".to_owned());

    let skin = b.add_skin(&model.skeleton);

    let mut mat_index: BTreeMap<String, json::Index<json::Material>> = BTreeMap::new();
    let mut scene_nodes: Vec<json::Index<json::Node>> = Vec::new();

    // Skeleton wrapper node carries the axis/scale transform; bone nodes hang
    // off it with Source-space local transforms.
    if let Some(s) = &skin {
        scene_nodes.push(s.root_node);
    }

    for part in &model.meshes {
        let mesh = b.add_mesh(part, &mut mat_index);
        let node = b.root.push(json::Node {
            mesh: Some(mesh),
            skin: skin.as_ref().map(|s| s.skin),
            matrix: Some(transform_source_to_gltf().m),
            name: Some(part.name.clone()),
            ..default_node()
        });
        scene_nodes.push(node);
    }

    b.root.push(json::Scene {
        nodes: scene_nodes,
        extensions: None,
        extras: Default::default(),
        name: None,
    });
    b.root.scene = Some(json::Index::new(0));

    b.finish()
}

/// Accumulates the single GLB binary buffer alongside the glTF document.
#[derive(Default)]
struct Builder {
    root: json::Root,
    bin: Vec<u8>,
}

struct SkinRefs {
    skin: json::Index<json::Skin>,
    root_node: json::Index<json::Node>,
}

impl Builder {
    /// Appends `bytes` as a new buffer view (4-byte aligned), returning its index.
    fn add_view(
        &mut self,
        bytes: &[u8],
        target: json::buffer::Target,
    ) -> json::Index<json::buffer::View> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64(bytes.len() as u64),
            byte_offset: Some(USize64(offset as u64)),
            byte_stride: None,
            target: Some(Valid(target)),
            name: None,
            extensions: None,
            extras: Default::default(),
        })
    }

    fn add_accessor(
        &mut self,
        view: json::Index<json::buffer::View>,
        count: usize,
        component_type: json::accessor::ComponentType,
        type_: json::accessor::Type,
        min_max: Option<(json::Value, json::Value)>,
    ) -> json::Index<json::Accessor> {
        let (min, max) = match min_max {
            Some((mn, mx)) => (Some(mn), Some(mx)),
            None => (None, None),
        };
        self.root.push(json::Accessor {
            buffer_view: Some(view),
            byte_offset: Some(USize64(0)),
            count: USize64(count as u64),
            component_type: Valid(json::accessor::GenericComponentType(component_type)),
            type_: Valid(type_),
            min,
            max,
            name: None,
            normalized: false,
            sparse: None,
            extensions: None,
            extras: Default::default(),
        })
    }

    /// Builds the model skin: a wrapper node, the bone hierarchy, and the
    /// inverse-bind-matrix accessor. Returns `None` for an empty skeleton.
    fn add_skin(&mut self, skeleton: &Skeleton) -> Option<SkinRefs> {
        if skeleton.bones.is_empty() {
            return None;
        }

        // One node per bone, with Source-space local translation + rotation.
        let bone_nodes: Vec<json::Index<json::Node>> = skeleton
            .bones
            .iter()
            .map(|bone| {
                self.root.push(json::Node {
                    translation: Some([bone.position.x, bone.position.y, bone.position.z]),
                    rotation: Some(json::scene::UnitQuaternion([
                        bone.rotation.x,
                        bone.rotation.y,
                        bone.rotation.z,
                        bone.rotation.w,
                    ])),
                    name: Some(bone.name.clone()),
                    ..default_node()
                })
            })
            .collect();

        // Wire children and collect roots.
        let mut children: Vec<Vec<json::Index<json::Node>>> =
            vec![Vec::new(); skeleton.bones.len()];
        let mut roots: Vec<json::Index<json::Node>> = Vec::new();
        for (i, bone) in skeleton.bones.iter().enumerate() {
            match bone.parent {
                Some(p) => children[p].push(bone_nodes[i]),
                None => roots.push(bone_nodes[i]),
            }
        }
        for (i, kids) in children.into_iter().enumerate() {
            if !kids.is_empty() {
                self.root.nodes[bone_nodes[i].value()].children = Some(kids);
            }
        }

        // Inverse-bind matrices, one MAT4 per bone, in bone-index order.
        let mut ibm = Vec::with_capacity(skeleton.bones.len() * 64);
        for bone in &skeleton.bones {
            for f in bone.inverse_bind.m {
                ibm.extend_from_slice(&f.to_le_bytes());
            }
        }
        let ibm_view = self.add_view(&ibm, json::buffer::Target::ArrayBuffer);
        let ibm_accessor = self.add_accessor(
            ibm_view,
            skeleton.bones.len(),
            json::accessor::ComponentType::F32,
            json::accessor::Type::Mat4,
            None,
        );

        let root_node = self.root.push(json::Node {
            children: Some(roots),
            matrix: Some(transform_source_to_gltf().m),
            name: Some("skeleton".to_owned()),
            ..default_node()
        });

        let skin = self.root.push(json::Skin {
            joints: bone_nodes,
            inverse_bind_matrices: Some(ibm_accessor),
            skeleton: Some(root_node),
            name: None,
            extensions: None,
            extras: Default::default(),
        });

        Some(SkinRefs { skin, root_node })
    }

    /// Builds one glTF mesh (its primitives + shared per-vertex-buffer accessors).
    fn add_mesh(
        &mut self,
        part: &MeshPart,
        mat_index: &mut BTreeMap<String, json::Index<json::Material>>,
    ) -> json::Index<json::Mesh> {
        // Deinterleaved attributes are shared by every primitive over a buffer.
        let vb_attrs: Vec<VertexAccessors> = part
            .vertex_buffers
            .iter()
            .map(|vb| self.add_vertex_buffer(vb, part.bone_weight_count))
            .collect();

        let mut primitives = Vec::with_capacity(part.primitives.len());
        for prim in &part.primitives {
            let attrs = &vb_attrs[prim.vertex_buffer];

            let mut attributes = BTreeMap::new();
            attributes.insert(Valid(json::mesh::Semantic::Positions), attrs.position);
            if let Some(a) = attrs.normal {
                attributes.insert(Valid(json::mesh::Semantic::Normals), a);
            }
            if let Some(a) = attrs.tangent {
                attributes.insert(Valid(json::mesh::Semantic::Tangents), a);
            }
            if let Some(a) = attrs.texcoord0 {
                attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), a);
            }
            if let Some(a) = attrs.joints0 {
                attributes.insert(Valid(json::mesh::Semantic::Joints(0)), a);
            }
            if let Some(a) = attrs.weights0 {
                attributes.insert(Valid(json::mesh::Semantic::Weights(0)), a);
            }

            let indices: Vec<u8> = prim.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let idx_view = self.add_view(&indices, json::buffer::Target::ElementArrayBuffer);
            let idx_accessor = self.add_accessor(
                idx_view,
                prim.indices.len(),
                json::accessor::ComponentType::U32,
                json::accessor::Type::Scalar,
                None,
            );

            let material = self.material_for(&prim.material, mat_index);

            primitives.push(json::mesh::Primitive {
                attributes,
                indices: Some(idx_accessor),
                material: Some(material),
                mode: Valid(json::mesh::Mode::Triangles),
                targets: None,
                extensions: None,
                extras: Default::default(),
            });
        }

        self.root.push(json::Mesh {
            primitives,
            weights: None,
            name: Some(part.name.clone()),
            extensions: None,
            extras: Default::default(),
        })
    }

    /// Writes one vertex buffer's attribute accessors. Skinned buffers with
    /// joints but no weights get VRF's default `1/bone_weight_count` spread.
    fn add_vertex_buffer(
        &mut self,
        vb: &VertexBuffer,
        bone_weight_count: usize,
    ) -> VertexAccessors {
        let count = vb.element_count;

        let pos_bytes: Vec<u8> = vb.positions.iter().flat_map(f32x).collect();
        let pos_view = self.add_view(&pos_bytes, json::buffer::Target::ArrayBuffer);
        let (min, max) = bounds(&vb.positions);
        let position = self.add_accessor(
            pos_view,
            count,
            json::accessor::ComponentType::F32,
            json::accessor::Type::Vec3,
            Some((min, max)),
        );

        let normal = (!vb.normals.is_empty()).then(|| {
            let bytes: Vec<u8> = vb.normals.iter().flat_map(f32x).collect();
            let view = self.add_view(&bytes, json::buffer::Target::ArrayBuffer);
            self.add_accessor(
                view,
                count,
                json::accessor::ComponentType::F32,
                json::accessor::Type::Vec3,
                None,
            )
        });
        let tangent = (!vb.tangents.is_empty()).then(|| {
            let bytes: Vec<u8> = vb.tangents.iter().flat_map(f32x).collect();
            let view = self.add_view(&bytes, json::buffer::Target::ArrayBuffer);
            self.add_accessor(
                view,
                count,
                json::accessor::ComponentType::F32,
                json::accessor::Type::Vec4,
                None,
            )
        });
        let texcoord0 = vb.texcoords.first().map(|uv| {
            let bytes: Vec<u8> = uv.iter().flat_map(f32x).collect();
            let view = self.add_view(&bytes, json::buffer::Target::ArrayBuffer);
            self.add_accessor(
                view,
                count,
                json::accessor::ComponentType::F32,
                json::accessor::Type::Vec2,
                None,
            )
        });

        let (joints0, weights0) = if vb.joints.is_empty() {
            (None, None)
        } else {
            let mut joints = vb.joints.clone();
            let mut weights = if vb.weights.is_empty() {
                default_weights(count, bone_weight_count)
            } else {
                vb.weights.clone()
            };
            fix_duplicate_joints(&mut joints, &mut weights);

            let jbytes: Vec<u8> = joints.iter().copied().flat_map(u16x).collect();
            let jview = self.add_view(&jbytes, json::buffer::Target::ArrayBuffer);
            let ja = self.add_accessor(
                jview,
                count,
                json::accessor::ComponentType::U16,
                json::accessor::Type::Vec4,
                None,
            );

            let wbytes: Vec<u8> = weights.iter().flat_map(f32x).collect();
            let wview = self.add_view(&wbytes, json::buffer::Target::ArrayBuffer);
            let wa = self.add_accessor(
                wview,
                count,
                json::accessor::ComponentType::F32,
                json::accessor::Type::Vec4,
                None,
            );

            (Some(ja), Some(wa))
        };

        VertexAccessors {
            position,
            normal,
            tangent,
            texcoord0,
            joints0,
            weights0,
        }
    }

    fn material_for(
        &mut self,
        path: &str,
        mat_index: &mut BTreeMap<String, json::Index<json::Material>>,
    ) -> json::Index<json::Material> {
        if let Some(i) = mat_index.get(path) {
            return *i;
        }
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        let index = self.root.push(json::Material {
            name: Some(name),
            pbr_metallic_roughness: json::material::PbrMetallicRoughness {
                base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
                metallic_factor: json::material::StrengthFactor(0.0),
                roughness_factor: json::material::StrengthFactor(1.0),
                base_color_texture: None,
                metallic_roughness_texture: None,
                extensions: None,
                extras: Default::default(),
            },
            ..Default::default()
        });
        mat_index.insert(path.to_owned(), index);
        index
    }

    /// Frames the document + binary buffer into a GLB byte stream.
    fn finish(mut self) -> Result<Vec<u8>, DecodeError> {
        self.root.push(json::Buffer {
            byte_length: USize64(self.bin.len() as u64),
            uri: None,
            name: None,
            extensions: None,
            extras: Default::default(),
        });

        let json_string = json::serialize::to_string(&self.root)
            .map_err(|_| DecodeError::Model("glTF JSON serialize failed"))?;
        let mut json_bytes = json_string.into_bytes();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }

        let total = 12 + 8 + json_bytes.len() + 8 + self.bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());

        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json_bytes);

        out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&self.bin);

        Ok(out)
    }
}

/// The accessors written for one vertex buffer, shared across its primitives.
struct VertexAccessors {
    position: json::Index<json::Accessor>,
    normal: Option<json::Index<json::Accessor>>,
    tangent: Option<json::Index<json::Accessor>>,
    texcoord0: Option<json::Index<json::Accessor>>,
    joints0: Option<json::Index<json::Accessor>>,
    weights0: Option<json::Index<json::Accessor>>,
}

fn default_node() -> json::Node {
    json::Node {
        camera: None,
        children: None,
        extensions: None,
        extras: Default::default(),
        matrix: None,
        mesh: None,
        name: None,
        rotation: None,
        scale: None,
        translation: None,
        skin: None,
        weights: None,
    }
}

fn f32x<const N: usize>(v: &[f32; N]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn u16x(v: [u16; 4]) -> Vec<u8> {
    v.iter().flat_map(|u| u.to_le_bytes()).collect()
}

fn bounds(positions: &[[f32; 3]]) -> (json::Value, json::Value) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    if !min[0].is_finite() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    (
        json::serialize::to_value(min.to_vec()).unwrap(),
        json::serialize::to_value(max.to_vec()).unwrap(),
    )
}

/// VRF's default weights for a mesh with joints but no weight stream:
/// `1/bone_weight_count` spread over the first `bone_weight_count` influences.
fn default_weights(count: usize, bone_weight_count: usize) -> Vec<[f32; 4]> {
    let bwc = bone_weight_count.clamp(1, 4);
    let w = 1.0 / bwc as f32;
    let mut row = [0.0f32; 4];
    for r in row.iter_mut().take(bwc) {
        *r = w;
    }
    vec![row; count]
}

/// Port of VRF `FixDuplicateJoints` (4-influence path): zero out influences with
/// zero weight, then merge duplicate joint indices, summing their weights.
fn fix_duplicate_joints(joints: &mut [[u16; 4]], weights: &mut [[f32; 4]]) {
    for (j, w) in joints.iter_mut().zip(weights.iter_mut()) {
        for k in 0..4 {
            if w[k] == 0.0 {
                j[k] = 0;
            }
        }
        // For each influence a, fold any later duplicate b into it (summing
        // weights) and shift the rest down. Walk both ends inward, as VRF does.
        for a in (0..=2).rev() {
            for b in (a + 1..=3).rev() {
                if j[a] == j[b] {
                    for l in b..3 {
                        j[l] = j[l + 1];
                        w[l] = w[l + 1];
                    }
                    j[3] = 0;
                    w[a] += w[b];
                    w[3] = 0.0;
                }
            }
        }
    }
}
