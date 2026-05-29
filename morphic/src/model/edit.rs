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
use super::glb;
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

pub(super) fn parse_embedded(
    bytes: &[u8],
) -> Result<(Resource<'_>, Vec<EmbeddedMesh>), DecodeError> {
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

/// Exports the vertex buffer at `block_index` as a standalone `.glb` for editing
/// in Blender (or any glTF tool): one triangle mesh with POSITION + NORMAL and a
/// custom `_ORIGID` per-vertex attribute carrying the original index. Reshape it
/// (displacement only, topology preserved) and feed the result back to
/// [`apply_edited_glb`].
pub fn export_buffer_for_edit(
    vmdl_bytes: &[u8],
    block_index: usize,
) -> Result<Vec<u8>, DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;
    let (em, local) = embedded
        .iter()
        .find_map(|em| {
            em.vertex_buffers
                .iter()
                .position(|d| d.block_index == block_index)
                .map(|local| (em, local))
        })
        .ok_or(DecodeError::Model("no vertex buffer at that block index"))?;

    // Assemble unskinned (remap None): we only need positions, normals, and the
    // draw-call triangulation for this buffer.
    let part = super::mesh::assemble(em, &resource, None)?;
    let vb = part.vertex_buffers.get(local).ok_or(DecodeError::Model(
        "buffer index out of range after assemble",
    ))?;
    let indices: Vec<u32> = part
        .primitives
        .iter()
        .filter(|p| p.vertex_buffer == local)
        .flat_map(|p| p.indices.iter().copied())
        .collect();

    glb::to_edit_glb(vb, &indices)
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

/// Split copies of one original vertex (Blender duplicates a vertex per
/// normal/UV seam, each carrying the same `_ORIGID`) must agree on position to
/// within this tolerance (source units = inches). A larger spread means a seam
/// was pulled apart, which displacement editing cannot represent.
const SPLIT_TOL: f32 = 1.0e-2;

/// Applies an edited `.glb` (as produced by [`export_buffer_for_edit`] and
/// reshaped in Blender) back onto the model: recovers each original vertex's new
/// position via the `_ORIGID` carrier and splices the buffer at `block_index`.
/// Topology must be preserved: every original vertex must be present exactly once
/// (after regrouping split copies), or this errors.
pub fn apply_edited_glb(
    vmdl_bytes: &[u8],
    block_index: usize,
    glb_bytes: &[u8],
) -> Result<Vec<u8>, DecodeError> {
    let (_resource, embedded) = parse_embedded(vmdl_bytes)?;
    let desc = find_vertex_buffer(&embedded, block_index)
        .ok_or(DecodeError::Model("no vertex buffer at that block index"))?;
    let new_positions = read_edited_positions(glb_bytes, desc.element_count)?;
    replace_vertex_positions(vmdl_bytes, block_index, &new_positions)
}

/// Reads an edited `.glb` and returns one position per original vertex, indexed by
/// `_ORIGID` (0..`count`). Positions are taken in node-world space so a baked
/// import/export axis transform is undone. Split copies are regrouped by id.
fn read_edited_positions(glb_bytes: &[u8], count: usize) -> Result<Vec<[f32; 3]>, DecodeError> {
    let (doc, buffers, _images) = gltf::import_slice(glb_bytes)
        .map_err(|_| DecodeError::Model("failed to parse edited glb"))?;
    let world = node_world_transforms(&doc);

    let mut acc: Vec<Option<[f32; 3]>> = vec![None; count];
    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let m = world[node.index()];
        for prim in mesh.primitives() {
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<[f32; 3]> = positions.collect();

            // Match the carrier tolerant of underscore handling: gltf-json wrote
            // it as `_ORIGID`, but a Blender import/export round-trip can shift the
            // leading underscores, so key off the "ORIGID" stem.
            let id_acc = prim
                .attributes()
                .find_map(|(sem, a)| match sem {
                    gltf::Semantic::Extras(name) if name.to_ascii_uppercase().contains("ORIGID") => {
                        Some(a)
                    }
                    _ => None,
                })
                .ok_or(DecodeError::Model(
                    "edited glb has no _ORIGID attribute (re-export with custom attributes enabled)",
                ))?;
            let ids = read_origid(&id_acc, &buffers)
                .ok_or(DecodeError::Model("could not read _ORIGID accessor"))?;
            if ids.len() != positions.len() {
                return Err(DecodeError::Model("_ORIGID / POSITION count mismatch"));
            }

            for (&id, &p) in ids.iter().zip(&positions) {
                let id = id as usize;
                if id >= count {
                    return Err(DecodeError::Model(
                        "_ORIGID out of range (topology changed?)",
                    ));
                }
                let pw = transform_point(&m, p);
                match acc[id] {
                    None => acc[id] = Some(pw),
                    Some(existing) => {
                        let spread = (0..3)
                            .map(|k| (existing[k] - pw[k]).abs())
                            .fold(0.0_f32, f32::max);
                        if spread > SPLIT_TOL {
                            return Err(DecodeError::Model(
                                "split vertices moved apart (a UV/normal seam was separated); \
                                 not supported by displacement editing",
                            ));
                        }
                    }
                }
            }
        }
    }

    acc.into_iter()
        .map(|p| {
            p.ok_or(DecodeError::Model(
                "edited glb is missing original vertices (topology changed: vertices removed)",
            ))
        })
        .collect()
}

/// Reads a SCALAR `_ORIGID` accessor as `u32` indices, handling float or integer
/// component types (Blender re-exports our float carrier as float).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn read_origid(acc: &gltf::Accessor, buffers: &[gltf::buffer::Data]) -> Option<Vec<u32>> {
    use gltf::accessor::{DataType, Dimensions};
    if acc.dimensions() != Dimensions::Scalar {
        return None;
    }
    let view = acc.view()?;
    let buf = buffers.get(view.buffer().index())?;
    let dt = acc.data_type();
    let comp = match dt {
        DataType::I8 | DataType::U8 => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::U32 | DataType::F32 => 4,
    };
    let stride = view.stride().unwrap_or(comp);
    let start = view.offset() + acc.offset();
    let mut out = Vec::with_capacity(acc.count());
    for i in 0..acc.count() {
        let o = start + i * stride;
        let id = match dt {
            DataType::F32 => {
                f32::from_le_bytes(buf.0.get(o..o + 4)?.try_into().ok()?).round() as u32
            }
            DataType::U32 => u32::from_le_bytes(buf.0.get(o..o + 4)?.try_into().ok()?),
            DataType::U16 => u32::from(u16::from_le_bytes(buf.0.get(o..o + 2)?.try_into().ok()?)),
            DataType::U8 => u32::from(*buf.0.get(o)?),
            // A signed carrier never occurs (we write F32); reject rather than
            // guess a reinterpretation.
            DataType::I8 | DataType::I16 => return None,
        };
        out.push(id);
    }
    Some(out)
}

/// World transform per node index (column-major 4x4), composing parent chains so
/// positions can be lifted out of any baked import/export axis transform.
fn node_world_transforms(doc: &gltf::Document) -> Vec<[[f32; 4]; 4]> {
    let mut out = vec![IDENTITY4; doc.nodes().count()];
    for scene in doc.scenes() {
        for node in scene.nodes() {
            accumulate(&node, IDENTITY4, &mut out);
        }
    }
    out
}

fn accumulate(node: &gltf::Node, parent: [[f32; 4]; 4], out: &mut [[[f32; 4]; 4]]) {
    let world = mat_mul(parent, node.transform().matrix());
    out[node.index()] = world;
    for child in node.children() {
        accumulate(&child, world, out);
    }
}

const IDENTITY4: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Column-major 4x4 multiply: `(a*b)[col][row] = sum_k a[k][row] * b[col][k]`.
#[allow(clippy::many_single_char_names)]
fn mat_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k][row] * b[col][k];
            }
            r[col][row] = s;
        }
    }
    r
}

/// Transforms a point by a column-major 4x4 (with implicit w=1).
#[allow(clippy::many_single_char_names)]
fn transform_point(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let v = [p[0], p[1], p[2], 1.0];
    let mut o = [0.0f32; 3];
    for (row, oo) in o.iter_mut().enumerate() {
        let mut s = 0.0;
        for (col, &vc) in v.iter().enumerate() {
            s += m[col][row] * vc;
        }
        *oo = s;
    }
    o
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

#[cfg(test)]
mod tests {
    // Positions round-trip through our own glTF writer/reader exactly, so the
    // tests assert exact float-array equality deliberately.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::model::VertexBuffer;

    fn quad() -> (VertexBuffer, Vec<u32>) {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let vb = VertexBuffer {
            element_count: 4,
            stride: 0,
            normals: vec![[0.0, 0.0, 1.0]; 4],
            positions,
            ..Default::default()
        };
        (vb, vec![0, 1, 2, 0, 2, 3])
    }

    /// `to_edit_glb` then `read_edited_positions` recovers the exact positions,
    /// in original vertex order, via the `_ORIGID` carrier (identity node
    /// transform, no Blender).
    #[test]
    fn edit_glb_round_trips_positions_by_id() {
        let (vb, indices) = quad();
        let glb = glb::to_edit_glb(&vb, &indices).expect("write edit glb");
        let recovered = read_edited_positions(&glb, vb.element_count).expect("read");
        assert_eq!(recovered, vb.positions);
    }

    /// A buffer with more original vertices than the glb carries (a vertex was
    /// removed in Blender) is rejected, not silently mis-spliced.
    #[test]
    fn edit_glb_missing_vertex_is_rejected() {
        let (vb, indices) = quad();
        let glb = glb::to_edit_glb(&vb, &indices).expect("write edit glb");
        // Ask for 5 originals when the glb only carries ids 0..=3.
        assert!(read_edited_positions(&glb, 5).is_err());
    }
}
