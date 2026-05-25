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
    let mdat = crate::kv3::parse(mdat_bytes)?;

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
            "BLENDINDICES"
                if remap.is_some()
                    || buf.fields.iter().any(|x| x.semantic_name == "BLENDWEIGHT") =>
            {
                out.joints = pack4(&buf.blend_indices(f, remap)?);
            }
            "BLENDWEIGHT" | "BLENDWEIGHTS" if remap.is_some() => {
                out.weights = buf.blend_weights(f)?;
            }
            _ => {}
        }
    }

    // A separately-stored tangent only applies when the normal did not already
    // carry one (uncompressed-normal meshes), matching VRF accessor precedence.
    if out.tangents.is_empty() {
        if let Some(t) = standalone_tangent {
            out.tangents = t;
        }
    }

    Ok(out)
}

/// Repacks a flat 4-per-vertex joint stream into `[u16; 4]` rows. Eight-bone
/// formats are out of scope for v1 (no Deadlock hero uses them).
fn pack4(flat: &[u16]) -> Vec<[u16; 4]> {
    flat.chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect()
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
