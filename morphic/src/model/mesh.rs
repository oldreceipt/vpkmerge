//! Mesh assembly: turns the `CTRL` embedded-mesh registry + per-mesh `MDAT`
//! draw calls + `MVTX`/`MIDX` buffers into in-memory primitives, following
//! VRF `Model.GetEmbeddedMeshes` (buffer registry), `Model.GetRemapTable`
//! (bone remap), and `GltfModelExporter` (draw call -> primitive).
//!
//! The structural readers ([`EmbeddedMesh::parse_all`], [`SceneObject::parse_all`])
//! work on parsed KV3 trees alone, so the bone/layout/draw-call structure is
//! testable without the multi-megabyte vertex buffers. [`assemble`] adds the
//! buffer decode + deinterleave once a block source is available.

// Scene bounds are stored as f64-widened f32 in KV3; narrowing back is exact.
#![allow(clippy::cast_possible_truncation)]

use crate::error::DecodeError;
use crate::kv3::Value;

use super::vbib::{BufferDesc, InputLayoutField, OnDiskBuffer};

/// One embedded mesh from `CTRL` `embedded_meshes` (the modern `MVTX`/`MIDX`
/// shape, keyed `m_Name` / `m_nDataBlock`).
#[derive(Debug, Clone)]
pub struct EmbeddedMesh {
    pub name: String,
    pub mesh_index: usize,
    /// Global block index of this mesh's `MDAT`.
    pub data_block: usize,
    pub vertex_buffers: Vec<BufferDesc>,
    pub index_buffers: Vec<BufferDesc>,
}

impl EmbeddedMesh {
    /// Parses every entry of `CTRL.embedded_meshes`. Errors if an entry uses
    /// the legacy `vbib_block` shape, which Deadlock hero models do not.
    pub fn parse_all(ctrl: &Value) -> Result<Vec<EmbeddedMesh>, DecodeError> {
        let arr = ctrl
            .get("embedded_meshes")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("CTRL has no embedded_meshes"))?;

        let mut out = Vec::with_capacity(arr.len());
        for em in arr {
            if em.get("vbib_block").is_some() || em.get("m_nDataBlock").is_none() {
                return Err(DecodeError::Model("unsupported embedded-mesh layout"));
            }
            let name = em
                .get("m_Name")
                .and_then(Value::as_str)
                .ok_or(DecodeError::Model("embedded mesh missing m_Name"))?
                .to_owned();
            let mesh_index = usize::try_from(
                em.get("m_nMeshIndex")
                    .and_then(Value::as_int)
                    .ok_or(DecodeError::Model("embedded mesh missing m_nMeshIndex"))?,
            )
            .map_err(|_| DecodeError::Model("negative mesh index"))?;
            let data_block = usize::try_from(
                em.get("m_nDataBlock")
                    .and_then(Value::as_int)
                    .ok_or(DecodeError::Model("embedded mesh missing m_nDataBlock"))?,
            )
            .map_err(|_| DecodeError::Model("negative data block"))?;

            let vertex_buffers = parse_buffer_list(em, "m_vertexBuffers")?;
            let index_buffers = parse_buffer_list(em, "m_indexBuffers")?;

            out.push(EmbeddedMesh {
                name,
                mesh_index,
                data_block,
                vertex_buffers,
                index_buffers,
            });
        }
        Ok(out)
    }
}

fn parse_buffer_list(em: &Value, key: &str) -> Result<Vec<BufferDesc>, DecodeError> {
    let arr = em
        .get(key)
        .and_then(Value::as_array)
        .ok_or(DecodeError::Model("embedded mesh missing a buffer list"))?;
    arr.iter().map(BufferDesc::from_kv).collect()
}

/// LOD-group masks from the model `DATA` block, one per embedded mesh (same
/// order as `CTRL.embedded_meshes`). A mesh belongs to LOD level `n` when
/// `mask & (1 << n)` is set; the golden render is LOD0.
pub fn lod_group_masks(data: &Value) -> Result<Vec<u64>, DecodeError> {
    let arr = data
        .get("m_refLODGroupMasks")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Model("DATA missing m_refLODGroupMasks"))?;
    arr.iter()
        .map(|v| {
            v.as_uint()
                .ok_or(DecodeError::Model("LOD mask not an integer"))
        })
        .collect()
}

/// Indices into `embedded` of the meshes drawn at LOD0.
pub fn lod0_indices(data: &Value, embedded: &[EmbeddedMesh]) -> Result<Vec<usize>, DecodeError> {
    let masks = lod_group_masks(data)?;
    Ok((0..embedded.len())
        .filter(|&i| masks.get(i).is_some_and(|m| m & 1 != 0))
        .collect())
}

/// One draw call within a scene object: a contiguous index range over one
/// vertex buffer, with its material.
#[derive(Debug, Clone)]
pub struct DrawCall {
    pub vertex_buffer: usize,
    pub index_buffer: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub start_index: usize,
    pub base_vertex: u32,
    pub material: String,
    pub primitive_type: String,
}

impl DrawCall {
    fn parse(dc: &Value) -> Result<DrawCall, DecodeError> {
        let vertex_buffer = dc
            .get("m_vertexBuffers")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|b| b.get("m_hBuffer"))
            .and_then(Value::as_int)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(DecodeError::Model("draw call missing vertex buffer handle"))?;
        let index_buffer = dc
            .get("m_indexBuffer")
            .and_then(|b| b.get("m_hBuffer"))
            .and_then(Value::as_int)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(DecodeError::Model("draw call missing index buffer handle"))?;
        let material = dc
            .get("m_material")
            .or_else(|| dc.get("m_pMaterial"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let primitive_type = dc
            .get("m_nPrimitiveType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        Ok(DrawCall {
            vertex_buffer,
            index_buffer,
            vertex_count: int_field(dc, "m_nVertexCount")?,
            index_count: int_field(dc, "m_nIndexCount")?,
            start_index: int_field(dc, "m_nStartIndex")?,
            base_vertex: u32::try_from(int_field(dc, "m_nBaseVertex")?)
                .map_err(|_| DecodeError::Model("base vertex too large"))?,
            material,
            primitive_type,
        })
    }
}

/// A scene object (one per mesh in practice): its draw calls plus the
/// precomputed source-space bounds VRF stores in `MDAT`.
#[derive(Debug, Clone)]
pub struct SceneObject {
    pub min_bounds: [f32; 3],
    pub max_bounds: [f32; 3],
    pub draw_calls: Vec<DrawCall>,
}

impl SceneObject {
    /// Parses every scene object + draw call from an `MDAT` KV3 tree.
    pub fn parse_all(mdat: &Value) -> Result<Vec<SceneObject>, DecodeError> {
        let objs = mdat
            .get("m_sceneObjects")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Model("MDAT has no m_sceneObjects"))?;

        let mut out = Vec::with_capacity(objs.len());
        for so in objs {
            let draw_calls = so
                .get("m_drawCalls")
                .and_then(Value::as_array)
                .ok_or(DecodeError::Model("scene object has no m_drawCalls"))?
                .iter()
                .map(DrawCall::parse)
                .collect::<Result<Vec<_>, _>>()?;
            out.push(SceneObject {
                min_bounds: read_vec3(so.get("m_vMinBounds")).unwrap_or([0.0; 3]),
                max_bounds: read_vec3(so.get("m_vMaxBounds")).unwrap_or([0.0; 3]),
                draw_calls,
            });
        }
        Ok(out)
    }
}

/// Bone-weight count from an `MDAT` mesh skeleton (`m_skeleton.m_nBoneWeightCount`);
/// 0 means the mesh is not skinned.
pub fn bone_weight_count(mdat: &Value) -> usize {
    mdat.get("m_skeleton")
        .and_then(|s| s.get("m_nBoneWeightCount"))
        .and_then(Value::as_int)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

/// A decoded, deinterleaved vertex buffer: one attribute array per semantic the
/// model actually carries. Empty vectors mean the attribute is absent.
#[derive(Debug, Clone, Default)]
pub struct VertexBuffer {
    pub element_count: usize,
    pub stride: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub texcoords: Vec<Vec<[f32; 2]>>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub layout: Vec<InputLayoutField>,
}

/// One primitive: a draw call's index range plus the buffer it draws from.
#[derive(Debug, Clone)]
pub struct Primitive {
    /// Index into the owning [`MeshPart::vertex_buffers`].
    pub vertex_buffer: usize,
    pub material: String,
    pub vertex_count: usize,
    /// Global indices into the vertex buffer (base vertex already applied).
    pub indices: Vec<u32>,
}

/// One LOD0 mesh: its decoded vertex buffers and the primitives over them.
#[derive(Debug, Clone)]
pub struct MeshPart {
    pub name: String,
    pub mesh_index: usize,
    pub vertex_buffers: Vec<VertexBuffer>,
    pub primitives: Vec<Primitive>,
    pub min_bounds: [f32; 3],
    pub max_bounds: [f32; 3],
    /// `MDAT m_skeleton.m_nBoneWeightCount`: influences per vertex. 0 = unskinned.
    /// Used to default weights for meshes that ship joints but no weights.
    pub bone_weight_count: usize,
}

/// Resolves a global block index to its bytes. Implemented by `Resource`; the
/// indirection keeps mesh assembly testable with a synthetic block source.
pub trait BlockSource {
    fn block(&self, index: usize) -> Option<&[u8]>;
}

/// Assembles one LOD0 mesh: decodes its buffers, deinterleaves attributes, and
/// builds primitives from its `MDAT` draw calls. `remap` maps mesh-local blend
/// indices to model skeleton bone indices.
pub fn assemble(
    embedded: &EmbeddedMesh,
    blocks: &dyn BlockSource,
    remap: Option<&[usize]>,
) -> Result<MeshPart, DecodeError> {
    let mdat_bytes = blocks
        .block(embedded.data_block)
        .ok_or(DecodeError::Model("MDAT block index out of range"))?;
    let mdat = crate::kv3::decode(mdat_bytes)?;

    let weight_count = bone_weight_count(&mdat);
    let skinned = weight_count > 0 && remap.is_some();

    // Decode + deinterleave each vertex buffer once.
    let mut vertex_buffers = Vec::with_capacity(embedded.vertex_buffers.len());
    for desc in &embedded.vertex_buffers {
        let raw = blocks
            .block(desc.block_index)
            .ok_or(DecodeError::Model("MVTX block index out of range"))?;
        let on_disk = desc.decode(raw, true)?;
        vertex_buffers.push(deinterleave(&on_disk, if skinned { remap } else { None })?);
    }

    // Decode index buffers (kept raw for per-draw-call slicing).
    let mut index_buffers = Vec::with_capacity(embedded.index_buffers.len());
    for desc in &embedded.index_buffers {
        let raw = blocks
            .block(desc.block_index)
            .ok_or(DecodeError::Model("MIDX block index out of range"))?;
        index_buffers.push(desc.decode(raw, false)?);
    }

    let scene_objects = SceneObject::parse_all(&mdat)?;
    let mut primitives = Vec::new();
    let mut min_bounds = [f32::INFINITY; 3];
    let mut max_bounds = [f32::NEG_INFINITY; 3];

    for so in &scene_objects {
        for i in 0..3 {
            min_bounds[i] = min_bounds[i].min(so.min_bounds[i]);
            max_bounds[i] = max_bounds[i].max(so.max_bounds[i]);
        }
        for dc in &so.draw_calls {
            if dc.primitive_type != "RENDER_PRIM_TRIANGLES" {
                return Err(DecodeError::Model("non-triangle primitive"));
            }
            let ib = index_buffers
                .get(dc.index_buffer)
                .ok_or(DecodeError::Model("draw call index buffer out of range"))?;
            if dc.vertex_buffer >= vertex_buffers.len() {
                return Err(DecodeError::Model("draw call vertex buffer out of range"));
            }
            let indices = ib.read_indices(dc.start_index, dc.index_count, dc.base_vertex)?;
            primitives.push(Primitive {
                vertex_buffer: dc.vertex_buffer,
                material: dc.material.clone(),
                vertex_count: dc.vertex_count,
                indices,
            });
        }
    }

    if !min_bounds[0].is_finite() {
        min_bounds = [0.0; 3];
        max_bounds = [0.0; 3];
    }

    Ok(MeshPart {
        name: embedded.name.clone(),
        mesh_index: embedded.mesh_index,
        vertex_buffers,
        primitives,
        min_bounds,
        max_bounds,
        bone_weight_count: weight_count,
    })
}

/// Deinterleaves one decoded vertex buffer into per-semantic attribute arrays,
/// following VRF's `CreateVertexBufferAccessors` attribute handling.
fn deinterleave(buf: &OnDiskBuffer, remap: Option<&[usize]>) -> Result<VertexBuffer, DecodeError> {
    let mut out = VertexBuffer {
        element_count: buf.element_count,
        stride: buf.element_size,
        layout: buf.fields.clone(),
        ..VertexBuffer::default()
    };

    // Stable order mirrors VRF: by semantic index, then byte offset.
    let mut fields: Vec<&InputLayoutField> = buf.fields.iter().collect();
    fields.sort_by(|a, b| {
        a.semantic_index
            .cmp(&b.semantic_index)
            .then(a.offset.cmp(&b.offset))
    });

    let mut standalone_tangent: Option<Vec<[f32; 4]>> = None;

    for f in fields {
        match f.semantic_name.as_str() {
            "POSITION" => out.positions = buf.positions()?,
            "NORMAL" => {
                let (normals, tangents) = buf.normal_tangent(f)?;
                out.normals = normals;
                if !tangents.is_empty() {
                    out.tangents = tangents;
                }
            }
            "TANGENT" => standalone_tangent = Some(buf.vector4(f)?),
            "TEXCOORD" => out.texcoords.push(buf.vector2(f)?),
            // BLENDINDICES + BLENDWEIGHT are reconciled together after the loop.
            _ => {}
        }
    }

    let (joints, weights) = decode_skinning(buf, remap)?;
    out.joints = joints;
    out.weights = weights;

    // A separately-stored tangent only applies when the normal did not already
    // carry one (uncompressed-normal meshes), matching VRF accessor precedence.
    if out.tangents.is_empty() {
        if let Some(t) = standalone_tangent {
            out.tangents = t;
        }
    }

    Ok(out)
}

/// Per-vertex skin influences reduced to glTF's 4-bone shape: joints paired
/// with their matching weights, one row per vertex.
type SkinAttrs = (Vec<[u16; 4]>, Vec<[f32; 4]>);

/// Decodes `BLENDINDICES` + `BLENDWEIGHT` into 4-influence-per-vertex joints and
/// weights. Source 2 hero meshes may carry up to 8 influences (an 8-wide
/// `BLENDINDICES` paired with an `R16G16B16A16_UNORM` weight stream of 8 `u8`s);
/// the glTF pipeline downstream is fixed at 4, so a vertex with more than 4
/// influences keeps its 4 highest-weight bones and renormalizes. Gating mirrors
/// the previous accessor handling: joints are emitted when the mesh is remapped
/// or carries weights; weights only when the mesh is actually skinned (remap
/// present).
fn decode_skinning(buf: &OnDiskBuffer, remap: Option<&[usize]>) -> Result<SkinAttrs, DecodeError> {
    let idx_field = buf.fields.iter().find(|f| f.semantic_name == "BLENDINDICES");
    let wt_field = buf
        .fields
        .iter()
        .find(|f| f.semantic_name == "BLENDWEIGHT" || f.semantic_name == "BLENDWEIGHTS");

    let want_joints = remap.is_some() || wt_field.is_some();
    let (Some(idx_field), true) = (idx_field, want_joints && buf.element_count > 0) else {
        return Ok((Vec::new(), Vec::new()));
    };

    let joints_flat = buf.blend_indices(idx_field, remap)?;
    let lanes = joints_flat.len() / buf.element_count;

    // Weights only when the mesh is actually skinned.
    let Some(wt_field) = wt_field.filter(|_| remap.is_some()) else {
        // Joints without weights: keep the first 4 influences in lane order.
        let joints = (0..buf.element_count)
            .map(|i| {
                let b = i * lanes;
                [joints_flat[b], joints_flat[b + 1], joints_flat[b + 2], joints_flat[b + 3]]
            })
            .collect();
        return Ok((joints, Vec::new()));
    };

    let weights_flat = buf.blend_weights(wt_field)?;
    if weights_flat.len() / buf.element_count != lanes {
        return Err(DecodeError::Model("BLENDINDICES/BLENDWEIGHT lane mismatch"));
    }

    let mut joints = Vec::with_capacity(buf.element_count);
    let mut weights = Vec::with_capacity(buf.element_count);
    for i in 0..buf.element_count {
        let base = i * lanes;
        let js = &joints_flat[base..base + lanes];
        let ws = &weights_flat[base..base + lanes];
        if lanes <= 4 {
            // 4-influence fast path: preserve lane order and the on-disk weights
            // verbatim (no reorder, no renormalize) so existing meshes stay
            // bit-identical.
            joints.push([js[0], js[1], js[2], js[3]]);
            weights.push([ws[0], ws[1], ws[2], ws[3]]);
        } else {
            let (j, w) = top4(js, ws);
            joints.push(j);
            weights.push(w);
        }
    }
    Ok((joints, weights))
}

/// Picks the 4 highest-weight influences of a >4-wide vertex and renormalizes
/// them to sum 1. The sort is stable, so equal weights keep their lane order.
fn top4(joints: &[u16], weights: &[f32]) -> ([u16; 4], [f32; 4]) {
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| {
        weights[b]
            .partial_cmp(&weights[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut j = [0u16; 4];
    let mut w = [0f32; 4];
    let mut sum = 0.0f32;
    for (slot, &lane) in order.iter().take(4).enumerate() {
        j[slot] = joints[lane];
        w[slot] = weights[lane];
        sum += weights[lane];
    }
    if sum > 0.0 {
        for x in &mut w {
            *x /= sum;
        }
    }
    (j, w)
}

fn int_field(o: &Value, key: &str) -> Result<usize, DecodeError> {
    let v = o
        .get(key)
        .and_then(Value::as_int)
        .ok_or(DecodeError::Model("draw call missing an integer field"))?;
    usize::try_from(v).map_err(|_| DecodeError::Model("negative draw-call field"))
}

fn read_vec3(v: Option<&Value>) -> Option<[f32; 3]> {
    let a = v?.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some([
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
    ])
}

#[cfg(test)]
mod skinning_tests {
    use super::super::dxgi::DxgiFormat;
    use super::*;

    fn field(name: &str, format: DxgiFormat, offset: usize) -> InputLayoutField {
        InputLayoutField {
            semantic_name: name.to_string(),
            semantic_index: 0,
            format,
            offset,
        }
    }

    /// An 8-influence vertex (Dynamo/Apollo shape) is reduced to its 4
    /// highest-weight bones, in descending order, renormalized to sum 1.
    #[test]
    fn eight_influence_keeps_top_four_and_renormalizes() {
        let mut data = vec![0u8; 16];
        data[0..8].copy_from_slice(&[10, 11, 12, 13, 14, 15, 16, 17]); // BLENDINDICES (8x u8)
        data[8..16].copy_from_slice(&[100, 60, 40, 30, 20, 5, 0, 0]); // BLENDWEIGHT (8x u8, sum 255)
        let buf = OnDiskBuffer {
            data,
            element_count: 1,
            element_size: 16,
            fields: vec![
                field("BLENDINDICES", DxgiFormat::R16G16B16A16Uint, 0),
                field("BLENDWEIGHT", DxgiFormat::R16G16B16A16Unorm, 8),
            ],
        };
        let remap: Vec<usize> = (0..32).collect();
        let (joints, weights) = decode_skinning(&buf, Some(&remap)).unwrap();

        assert_eq!(joints, vec![[10, 11, 12, 13]]);
        let w = weights[0];
        let total: f32 = w.iter().sum();
        assert!((total - 1.0).abs() < 1e-6, "renormalized to 1: {w:?}");
        let expect = [100.0 / 230.0, 60.0 / 230.0, 40.0 / 230.0, 30.0 / 230.0];
        for (a, b) in w.iter().zip(expect) {
            assert!((a - b).abs() < 1e-6, "{w:?} vs {expect:?}");
        }
    }

    /// A 4-influence vertex passes through untouched: lane order preserved and
    /// the on-disk weights kept verbatim (no reorder, no renormalize).
    #[test]
    fn four_influence_passes_through_unchanged() {
        let mut data = vec![0u8; 8];
        data[0..4].copy_from_slice(&[5, 6, 7, 8]); // BLENDINDICES R8G8B8A8_UINT
        data[4..8].copy_from_slice(&[128, 64, 63, 0]); // BLENDWEIGHT R8G8B8A8_UNORM (sum 255)
        let buf = OnDiskBuffer {
            data,
            element_count: 1,
            element_size: 8,
            fields: vec![
                field("BLENDINDICES", DxgiFormat::R8G8B8A8Uint, 0),
                field("BLENDWEIGHT", DxgiFormat::R8G8B8A8Unorm, 4),
            ],
        };
        let remap: Vec<usize> = (0..16).collect();
        let (joints, weights) = decode_skinning(&buf, Some(&remap)).unwrap();

        assert_eq!(joints, vec![[5, 6, 7, 8]]);
        let expect = [128.0 / 255.0, 64.0 / 255.0, 63.0 / 255.0, 0.0];
        for (a, b) in weights[0].iter().zip(expect) {
            assert!((a - b).abs() < 1e-6, "{:?} vs {expect:?}", weights[0]);
        }
    }
}
