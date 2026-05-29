//! Tier-1 topology edits: change *which* geometry a model renders, as opposed to
//! the Tier-0 vertex displacement in [`super::edit`] (which moves existing
//! vertices without changing the draw-call set).
//!
//! The first operation is **remove**: drop the draw call(s) for a given material
//! so that garment/part stops rendering. Removing a draw call touches only the
//! `MDAT` KV3 block (`m_sceneObjects[].m_drawCalls[]`); the shared `MVTX`/`MIDX`
//! buffers and every *surviving* draw call's index range are left untouched (the
//! removed call's slice of the index buffer simply goes unreferenced), so no
//! buffer re-encode or index renumbering is needed. The edited `MDAT` is
//! re-encoded as uncompressed KV3 v4 (the engine-accepted writer) and spliced
//! back with [`Resource::rebuild_with_block`].
//!
//! This is also the cheapest probe of the model-KV3 rewrite path: Tier 0 only
//! ever re-encoded `MVTX`, never a model KV3 block. See
//! `docs/handoff-model-edit.md` (T1a).

use crate::error::DecodeError;
use crate::kv3::{self, Value};
use crate::resource::Resource;

use super::edit::parse_embedded;
use super::mesh;

/// One draw call in a model, located by the blocks/indices needed to address it.
#[derive(Debug, Clone)]
pub struct DrawCallInfo {
    /// Owning embedded mesh name (e.g. `body`, `gun`).
    pub mesh_name: String,
    pub mesh_index: usize,
    /// Global block index of the `MDAT` this draw call lives in.
    pub data_block: usize,
    /// Index into `MDAT.m_sceneObjects`.
    pub scene_object: usize,
    /// Index into that scene object's `m_drawCalls`.
    pub draw_call: usize,
    /// Material path the draw call renders with (`m_material`), e.g.
    /// `models/.../vindicta_dress.vmat`.
    pub material: String,
    pub vertex_count: usize,
    pub index_count: usize,
}

/// One draw call dropped by [`remove_draw_calls_by_material`].
#[derive(Debug, Clone)]
pub struct RemovedDrawCall {
    pub mesh_name: String,
    pub data_block: usize,
    pub material: String,
    pub vertex_count: usize,
    pub index_count: usize,
}

/// Lists the renderable (LOD0) draw calls of a `.vmdl_c`, with the material each
/// renders and where it sits. Use it to discover the exact material string to
/// pass to [`remove_draw_calls_by_material`].
///
/// Only LOD0 is listed (the set the player sees up close), to keep the output
/// readable; [`remove_draw_calls_by_material`] still removes a matching material
/// from *every* LOD so the part disappears at all distances.
pub fn draw_call_targets(vmdl_bytes: &[u8]) -> Result<Vec<DrawCallInfo>, DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;
    let data = kv3::decode(resource.data_block()?)?;
    let lod0 = mesh::lod0_indices(&data, &embedded)?;

    let mut out = Vec::new();
    for &i in &lod0 {
        let em = &embedded[i];
        let mdat_bytes = resource
            .get_block_by_index(em.data_block)
            .ok_or(DecodeError::Model("MDAT block index out of range"))?;
        let mdat = kv3::decode(mdat_bytes)?;
        let Some(scene_objects) = mdat.get("m_sceneObjects").and_then(Value::as_array) else {
            continue;
        };
        for (so_i, so) in scene_objects.iter().enumerate() {
            let Some(draw_calls) = so.get("m_drawCalls").and_then(Value::as_array) else {
                continue;
            };
            for (dc_i, dc) in draw_calls.iter().enumerate() {
                out.push(DrawCallInfo {
                    mesh_name: em.name.clone(),
                    mesh_index: em.mesh_index,
                    data_block: em.data_block,
                    scene_object: so_i,
                    draw_call: dc_i,
                    material: material_of(dc).to_owned(),
                    vertex_count: usize_field(dc, "m_nVertexCount"),
                    index_count: usize_field(dc, "m_nIndexCount"),
                });
            }
        }
    }
    Ok(out)
}

/// Stops every draw call whose material path contains `needle` (case-insensitive
/// substring) from rendering, across all of the model's `MDAT` blocks (every LOD),
/// and splices the edited blocks back. Returns the new `.vmdl_c` bytes and a list
/// of what was neutralized.
///
/// The draw call is **not** deleted from the array (that would force a lossy KV3
/// re-encode the engine's model loader rejects: it drops value flags and
/// auxiliary-buffer typed-array tags that `MDAT` relies on). Instead the `MDAT`
/// block is re-wrapped uncompressed but otherwise byte-for-byte (preserving all of
/// that structure) and the matching draw calls' `m_nIndexCount` is zeroed in place,
/// so they submit no primitives. The shared vertex/index buffers and every other
/// draw call are untouched; per-scene-object bounds stay a (still-valid) superset.
///
/// Errors with [`DecodeError::Model`] if `needle` matches no draw call, so a
/// typo'd material name fails loudly instead of repacking the model unchanged.
pub fn remove_draw_calls_by_material(
    vmdl_bytes: &[u8],
    needle: &str,
) -> Result<(Vec<u8>, Vec<RemovedDrawCall>), DecodeError> {
    let needle_lc = needle.to_ascii_lowercase();
    let matches = |material: &str| material.to_ascii_lowercase().contains(&needle_lc);

    let (resource, embedded) = parse_embedded(vmdl_bytes)?;

    // Edit each distinct MDAT once, collecting (block_index, new_bytes).
    let mut edits: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut removed: Vec<RemovedDrawCall> = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    for em in &embedded {
        if done.contains(&em.data_block) {
            continue;
        }
        done.push(em.data_block);

        let mdat_bytes = resource
            .get_block_by_index(em.data_block)
            .ok_or(DecodeError::Model("MDAT block index out of range"))?;
        let tree = kv3::decode(mdat_bytes)?;
        let found = find_matching_draw_calls(&tree, &matches);
        if found.is_empty() {
            continue;
        }

        let targets: Vec<(usize, usize)> = found
            .iter()
            .map(|f| (f.scene_object, f.draw_call))
            .collect();
        let patched = kv3::neutralize_draw_calls(mdat_bytes, &targets)?;
        for f in found {
            removed.push(RemovedDrawCall {
                mesh_name: em.name.clone(),
                data_block: em.data_block,
                material: f.material,
                vertex_count: f.vertex_count,
                index_count: f.index_count,
            });
        }
        edits.push((em.data_block, patched));
    }

    if edits.is_empty() {
        return Err(DecodeError::Model(
            "no draw call matched the given material",
        ));
    }

    // Apply each MDAT swap. `rebuild_with_block` preserves block order/count, so
    // the indices collected from the original parse stay valid across rebuilds.
    let mut bytes = vmdl_bytes.to_vec();
    for (block_index, new_mdat) in &edits {
        let resource = Resource::parse(&bytes)?;
        bytes = resource.rebuild_with_block(*block_index, new_mdat)?;
    }
    Ok((bytes, removed))
}

/// Diagnostic: re-emits every distinct `MDAT` block **uncompressed but otherwise
/// byte-faithful** (via [`kv3::rewrap_uncompressed`], preserving value flags and
/// typed-array tags), then splices them all back. The decoded tree is identical to
/// the input; its only purpose is the in-game probe "does the engine accept our
/// re-emitted model KV3 blocks at all?" independent of any edit. If even this fails
/// to load, the problem is the resource container rebuild, not the KV3 encoding.
/// Returns the new bytes and the number of `MDAT` blocks re-emitted.
pub fn reencode_all_mdat_identity(vmdl_bytes: &[u8]) -> Result<(Vec<u8>, usize), DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;
    let mut edits: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    for em in &embedded {
        if done.contains(&em.data_block) {
            continue;
        }
        done.push(em.data_block);
        let mdat_bytes = resource
            .get_block_by_index(em.data_block)
            .ok_or(DecodeError::Model("MDAT block index out of range"))?;
        edits.push((em.data_block, kv3::rewrap_uncompressed(mdat_bytes)?));
    }
    let count = edits.len();
    let mut bytes = vmdl_bytes.to_vec();
    for (block_index, new_mdat) in &edits {
        let resource = Resource::parse(&bytes)?;
        bytes = resource.rebuild_with_block(*block_index, new_mdat)?;
    }
    Ok((bytes, count))
}

/// One draw call located by [`find_matching_draw_calls`]: its `(scene_object,
/// draw_call)` indices within the `MDAT` plus the info reported back to the caller.
pub(super) struct FoundDrawCall {
    pub scene_object: usize,
    pub draw_call: usize,
    pub material: String,
    pub vertex_count: usize,
    pub index_count: usize,
}

/// Locates every draw call matching `matches` in an `MDAT` tree, returning its
/// `m_sceneObjects`/`m_drawCalls` indices (for [`kv3::neutralize_draw_calls`]) and
/// the reportable counts. Read-only: nothing is mutated. Split out so the offline
/// fixture test can exercise it on a bare `MDAT` tree without a full `.vmdl_c`.
pub(super) fn find_matching_draw_calls(
    mdat: &Value,
    matches: &impl Fn(&str) -> bool,
) -> Vec<FoundDrawCall> {
    let mut out = Vec::new();
    let Some(scene_objects) = mdat.get("m_sceneObjects").and_then(Value::as_array) else {
        return out;
    };
    for (so_i, so) in scene_objects.iter().enumerate() {
        let Some(draw_calls) = so.get("m_drawCalls").and_then(Value::as_array) else {
            continue;
        };
        for (dc_i, dc) in draw_calls.iter().enumerate() {
            let material = material_of(dc);
            if matches(material) {
                out.push(FoundDrawCall {
                    scene_object: so_i,
                    draw_call: dc_i,
                    material: material.to_owned(),
                    vertex_count: usize_field(dc, "m_nVertexCount"),
                    index_count: usize_field(dc, "m_nIndexCount"),
                });
            }
        }
    }
    out
}

/// The material path a draw call renders with, reading either the modern
/// `m_material` or the legacy `m_pMaterial` key (mirrors `mesh::DrawCall::parse`).
fn material_of(dc: &Value) -> &str {
    dc.get("m_material")
        .or_else(|| dc.get("m_pMaterial"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Reads an unsigned draw-call field, defaulting to 0 when absent (the value is
/// informational, used only for the removal report).
fn usize_field(dc: &Value, key: &str) -> usize {
    dc.get(key)
        .and_then(Value::as_int)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}
