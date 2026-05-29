//! Vertex-displacement model edits (Tier 0): reshape existing geometry by
//! rewriting the `POSITION` lane of a mesh's vertex buffer, without changing
//! topology. The model-world analog of the texture splice in `crate::edit`:
//! decode the `MVTX` block, overwrite positions, re-encode in the same
//! meshoptimizer codec, and splice it back into the resource container.
//!
//! Vertex buffers are addressed by their global block index (the `m_nBlockIndex`
//! the `CTRL` buffer registry stores), enumerated by [`vertex_targets`].

use crate::error::DecodeError;
use crate::meshopt::encode_vertex_buffer;
use crate::resource::Resource;

use super::dxgi::DxgiFormat;
use super::mesh::EmbeddedMesh;
use super::vbib::BufferDesc;

const CTRL: [u8; 4] = *b"CTRL";

/// One editable (or inspectable) vertex buffer in a `.vmdl_c`.
#[derive(Debug, Clone)]
pub struct VertexTarget {
    /// The owning embedded mesh's name (e.g. `body`, `gun`).
    pub mesh_name: String,
    pub mesh_index: usize,
    /// Global block index of this buffer's `MVTX` payload; the handle passed to
    /// [`read_vertex_positions`] / [`replace_vertex_positions`].
    pub block_index: usize,
    pub vertex_count: usize,
    pub stride: usize,
    pub meshopt: bool,
    /// True when this buffer can be displacement-edited: meshopt-compressed,
    /// not ZSTD, and carrying an `R32G32B32_FLOAT` `POSITION`.
    pub editable: bool,
}

fn parse_embedded(bytes: &[u8]) -> Result<(Resource<'_>, Vec<EmbeddedMesh>), DecodeError> {
    let resource = Resource::parse(bytes)?;
    let ctrl_bytes = resource
        .find_block(CTRL)
        .ok_or(DecodeError::Model("model has no CTRL block"))?;
    let ctrl = crate::kv3::decode(ctrl_bytes)?;
    let embedded = EmbeddedMesh::parse_all(&ctrl)?;
    Ok((resource, embedded))
}

fn find_vertex_buffer(embedded: &[EmbeddedMesh], block_index: usize) -> Option<&BufferDesc> {
    embedded
        .iter()
        .flat_map(|em| &em.vertex_buffers)
        .find(|d| d.block_index == block_index)
}

fn has_float_position(desc: &BufferDesc) -> bool {
    desc.fields
        .iter()
        .any(|f| f.semantic_name == "POSITION" && f.format == DxgiFormat::R32G32B32Float)
}

/// Lists every vertex buffer in a `.vmdl_c`, with the block index used to edit
/// it and whether a displacement edit is supported.
pub fn vertex_targets(vmdl_bytes: &[u8]) -> Result<Vec<VertexTarget>, DecodeError> {
    let (_resource, embedded) = parse_embedded(vmdl_bytes)?;
    let mut out = Vec::new();
    for em in &embedded {
        for d in &em.vertex_buffers {
            out.push(VertexTarget {
                mesh_name: em.name.clone(),
                mesh_index: em.mesh_index,
                block_index: d.block_index,
                vertex_count: d.element_count,
                stride: d.element_size,
                meshopt: d.meshopt,
                editable: d.meshopt && !d.zstd && has_float_position(d),
            });
        }
    }
    Ok(out)
}

/// Reads the current `POSITION` array of the vertex buffer at `block_index`,
/// in the buffer's native vertex order. Pair with [`replace_vertex_positions`]
/// to apply a transform: read, transform each position, write back.
pub fn read_vertex_positions(
    vmdl_bytes: &[u8],
    block_index: usize,
) -> Result<Vec<[f32; 3]>, DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;
    let desc = find_vertex_buffer(&embedded, block_index)
        .ok_or(DecodeError::Model("no vertex buffer at that block index"))?;
    let raw = resource
        .get_block_by_index(block_index)
        .ok_or(DecodeError::Model("MVTX block index out of range"))?;
    let on_disk = desc.decode(raw, true)?;
    on_disk.positions()
}

/// Replaces the `POSITION` lane of the vertex buffer at `block_index` with
/// `new_positions` (same count, same order), re-encodes the buffer in its native
/// meshoptimizer codec, and splices it back, returning the new `.vmdl_c` bytes.
/// All other vertex attributes, every other block, and the model topology are
/// preserved.
///
/// Errors if the buffer is not meshopt-compressed (only those can be re-encoded),
/// is ZSTD, lacks a float `POSITION`, or if `new_positions.len()` differs from
/// the vertex count.
pub fn replace_vertex_positions(
    vmdl_bytes: &[u8],
    block_index: usize,
    new_positions: &[[f32; 3]],
) -> Result<Vec<u8>, DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;
    let desc = find_vertex_buffer(&embedded, block_index)
        .ok_or(DecodeError::Model("no vertex buffer at that block index"))?;
    if !desc.meshopt {
        return Err(DecodeError::Model(
            "only meshopt-compressed vertex buffers can be re-encoded",
        ));
    }
    if desc.zstd {
        return Err(DecodeError::Model("ZSTD vertex buffers not supported"));
    }

    let raw = resource
        .get_block_by_index(block_index)
        .ok_or(DecodeError::Model("MVTX block index out of range"))?;
    let mut on_disk = desc.decode(raw, true)?;
    on_disk.write_positions(new_positions)?;

    let new_mvtx = encode_vertex_buffer(desc.element_count, desc.element_size, &on_disk.data)?;
    resource.rebuild_with_block(block_index, &new_mvtx)
}
