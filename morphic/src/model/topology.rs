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
use crate::kv3::{self, Seg, Value};
use crate::resource::Resource;

use super::edit::{build_mesh_buffers_to_layout, parse_embedded, EncodedMesh};
use super::mesh::{self, VertexBuffer};
use super::skeleton;
use super::vbib::BufferDesc;

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

// --- T1d-d: replace one mesh part's geometry in place (the wedge) ---

/// What [`replace_mesh_part`] swapped, for the CLI report.
#[derive(Debug, Clone)]
pub struct ReplacedMeshPart {
    pub mesh_name: String,
    pub material: String,
    pub old_vertex_count: usize,
    pub new_vertex_count: usize,
    pub old_index_count: usize,
    pub new_index_count: usize,
    /// New vertex stride in bytes.
    pub stride: usize,
    /// New index width in bytes (2 or 4).
    pub index_size: usize,
}

/// Replaces an existing mesh part's geometry with a brand-new mesh of *any*
/// vertex/index count, **in place** (the T1d wedge from `docs/handoff-model-edit.md`).
/// Unlike a true additive insert (deferred), this reuses the target part's block
/// slots, material, and bone palette, so it needs only block *swaps* plus in-place
/// *scalar* edits, never KV3 array growth or a container block-table grow:
///
/// 1. Localize the new mesh's model-space `JOINTS_0` into the target mesh's bone
///    palette (T1d-c), so the existing remap table needs no edit.
/// 2. Assemble + encode the new buffer to the target's exact `m_inputLayoutFields`
///    field set, uncompressed (T1d-b), so the layout's element count is unchanged.
/// 3. Swap the `MVTX`/`MIDX` blocks (reusing their block indices).
/// 4. `set_scalars` the `CTRL` registry (buffer element counts, stride, each
///    field's format/offset, index width) and the `MDAT` draw call (vertex/index
///    counts, start index 0, base vertex 0).
///
/// The target part must have exactly one vertex buffer, one index buffer, and one
/// draw call (the clean wedge; the gun part fits). The new mesh is skinned against
/// the model skeleton (glTF joint indices == model bone indices); a skinned new
/// mesh requires the target to have a bone remap table. Errors loudly on any of
/// these contract violations rather than producing a broken model.
///
/// Returns the new `.vmdl_c` bytes and a report of what changed.
pub fn replace_mesh_part(
    vmdl_bytes: &[u8],
    mesh_name: &str,
    new_mesh: &VertexBuffer,
    indices: &[u32],
) -> Result<(Vec<u8>, ReplacedMeshPart), DecodeError> {
    let (resource, embedded) = parse_embedded(vmdl_bytes)?;

    let mesh_pos = embedded
        .iter()
        .position(|em| em.name == mesh_name)
        .ok_or(DecodeError::Model("no embedded mesh with that name"))?;
    let em = &embedded[mesh_pos];
    if em.vertex_buffers.len() != 1 || em.index_buffers.len() != 1 {
        return Err(DecodeError::Model(
            "replace-in-place needs a mesh part with exactly one vertex buffer and one index \
             buffer (LOD-split parts like the body are not supported by the wedge)",
        ));
    }
    let vb_desc = &em.vertex_buffers[0];
    let ib_desc = &em.index_buffers[0];
    let mdat_block = em.data_block;
    let vb_block = vb_desc.block_index;
    let ib_block = ib_desc.block_index;
    let old_vertex_count = vb_desc.element_count;
    let old_index_count = ib_desc.element_count;

    // Localize the new mesh's JOINTS_0 into the target mesh's bone palette (T1d-c).
    let data = kv3::decode(resource.data_block()?)?;
    let remap = skeleton::remap_table(&data, em.mesh_index);
    let mut local = new_mesh.clone();
    if !new_mesh.joints.is_empty() {
        let table = remap.as_deref().ok_or(DecodeError::Model(
            "new mesh is skinned but the target mesh has no bone remap table",
        ))?;
        let weights = (new_mesh.weights.len() == new_mesh.element_count)
            .then_some(new_mesh.weights.as_slice());
        local.joints = skeleton::localize_joints(&new_mesh.joints, weights, table)?;
    }

    // Assemble to the target's field set + encode both buffers (T1d-b).
    let enc = build_mesh_buffers_to_layout(&local, indices, &vb_desc.fields)?;
    let new_vertex_count = enc.vertex_count;
    let new_index_count = enc.index_count;
    let stride = enc.stride;
    let index_size = enc.index_size;

    // The single draw call this part renders (the clean-wedge contract).
    let mdat_bytes = resource
        .get_block_by_index(mdat_block)
        .ok_or(DecodeError::Model("MDAT block index out of range"))?;
    let mdat = kv3::decode(mdat_bytes)?;
    let (so_idx, dc_idx, material) = locate_single_draw_call(&mdat)?;
    let draw_call = mdat
        .get("m_sceneObjects")
        .and_then(Value::as_array)
        .and_then(|a| a.get(so_idx))
        .and_then(|so| so.get("m_drawCalls"))
        .and_then(Value::as_array)
        .and_then(|a| a.get(dc_idx))
        .ok_or(DecodeError::Model("draw call vanished after locate"))?;

    // Compute the scalar edits for the buffer registry (CTRL) and draw call (MDAT).
    let ctrl_idx = resource
        .blocks()
        .iter()
        .position(|b| &b.kind == b"CTRL")
        .ok_or(DecodeError::Model("model has no CTRL block"))?;
    let ctrl_bytes = resource
        .find_block(*b"CTRL")
        .ok_or(DecodeError::Model("model has no CTRL block"))?;
    let ctrl_edits = ctrl_edits_for(mesh_pos, vb_desc, ib_desc, &enc)?;
    let mdat_edits = mdat_edits_for(so_idx, dc_idx, draw_call, new_vertex_count, new_index_count)?;

    // Splice the changed blocks. Block order/count is preserved, so block indices
    // (and the CTRL references to them) stay valid across each rebuild. CTRL/MDAT
    // are only re-emitted when they actually changed (an unchanged-count replace
    // touches only MVTX/MIDX, like a Tier-0 edit).
    let mut swaps: Vec<(usize, Vec<u8>)> = Vec::with_capacity(4);
    if !ctrl_edits.is_empty() {
        swaps.push((ctrl_idx, kv3::set_scalars(ctrl_bytes, &ctrl_edits)?));
    }
    if !mdat_edits.is_empty() {
        swaps.push((mdat_block, kv3::set_scalars(mdat_bytes, &mdat_edits)?));
    }
    swaps.push((vb_block, enc.mvtx));
    swaps.push((ib_block, enc.midx));

    let mut bytes = vmdl_bytes.to_vec();
    for (idx, payload) in swaps {
        let res = Resource::parse(&bytes)?;
        bytes = res.rebuild_with_block(idx, &payload)?;
    }

    Ok((
        bytes,
        ReplacedMeshPart {
            mesh_name: mesh_name.to_string(),
            material,
            old_vertex_count,
            new_vertex_count,
            old_index_count,
            new_index_count,
            stride,
            index_size,
        },
    ))
}

/// The `CTRL` buffer-registry scalar edits for replacing mesh part `mesh_pos`'s
/// single vertex/index buffer with `enc`: new element counts, vertex stride, index
/// width, and each layout field's format/offset. Field count + order are unchanged
/// (so no KV3 array growth), and only values that actually differ are emitted (a
/// KV3 `0`/`1` is a tagless constant with no byte to set; the first field's offset
/// is always 0, and the gun's formats are already uncompressed float).
fn ctrl_edits_for(
    mesh_pos: usize,
    vb_desc: &BufferDesc,
    ib_desc: &BufferDesc,
    enc: &EncodedMesh,
) -> Result<Vec<(Vec<Seg>, i64)>, DecodeError> {
    let vb_path = vec![
        seg("embedded_meshes"),
        Seg::Index(mesh_pos),
        seg("m_vertexBuffers"),
        Seg::Index(0),
    ];
    let ib_path = vec![
        seg("embedded_meshes"),
        Seg::Index(mesh_pos),
        seg("m_indexBuffers"),
        Seg::Index(0),
    ];
    let mut edits: Vec<(Vec<Seg>, i64)> = Vec::new();
    push_if_changed(
        &mut edits,
        child(&vb_path, "m_nElementCount"),
        vb_desc.element_count,
        enc.vertex_count,
    )?;
    push_if_changed(
        &mut edits,
        child(&vb_path, "m_nElementSizeInBytes"),
        vb_desc.element_size,
        enc.stride,
    )?;
    push_if_changed(
        &mut edits,
        child(&ib_path, "m_nElementCount"),
        ib_desc.element_count,
        enc.index_count,
    )?;
    push_if_changed(
        &mut edits,
        child(&ib_path, "m_nElementSizeInBytes"),
        ib_desc.element_size,
        enc.index_size,
    )?;
    for (fi, f) in enc.fields.iter().enumerate() {
        let mut field_path = vb_path.clone();
        field_path.push(seg("m_inputLayoutFields"));
        field_path.push(Seg::Index(fi));
        let old = &vb_desc.fields[fi];
        if f.format.id() != old.format.id() {
            edits.push((child(&field_path, "m_Format"), i64::from(f.format.id())));
        }
        push_if_changed(
            &mut edits,
            child(&field_path, "m_nOffset"),
            old.offset,
            f.offset,
        )?;
    }
    Ok(edits)
}

/// The `MDAT` draw-call scalar edits: the replacement spans the whole new buffer
/// from the start, so vertex/index counts are updated and start index / base
/// vertex are pinned to 0 (only emitted when they differ from the current value).
fn mdat_edits_for(
    so_idx: usize,
    dc_idx: usize,
    draw_call: &Value,
    new_vertex_count: usize,
    new_index_count: usize,
) -> Result<Vec<(Vec<Seg>, i64)>, DecodeError> {
    let dc_path = vec![
        seg("m_sceneObjects"),
        Seg::Index(so_idx),
        seg("m_drawCalls"),
        Seg::Index(dc_idx),
    ];
    let mut edits: Vec<(Vec<Seg>, i64)> = Vec::new();
    push_if_changed(
        &mut edits,
        child(&dc_path, "m_nVertexCount"),
        dc_uint(draw_call, "m_nVertexCount"),
        new_vertex_count,
    )?;
    push_if_changed(
        &mut edits,
        child(&dc_path, "m_nIndexCount"),
        dc_uint(draw_call, "m_nIndexCount"),
        new_index_count,
    )?;
    push_if_changed(
        &mut edits,
        child(&dc_path, "m_nStartIndex"),
        dc_uint(draw_call, "m_nStartIndex"),
        0,
    )?;
    push_if_changed(
        &mut edits,
        child(&dc_path, "m_nBaseVertex"),
        dc_uint(draw_call, "m_nBaseVertex"),
        0,
    )?;
    Ok(edits)
}

/// Locates the single draw call of a mesh part's `MDAT`, returning its
/// `(scene_object, draw_call)` indices and material. Errors if the part has zero
/// or more than one draw call (the wedge maps the new mesh onto exactly one).
fn locate_single_draw_call(mdat: &Value) -> Result<(usize, usize, String), DecodeError> {
    let scene_objects = mdat
        .get("m_sceneObjects")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Model("MDAT has no m_sceneObjects"))?;
    let mut found: Option<(usize, usize, String)> = None;
    for (so_i, so) in scene_objects.iter().enumerate() {
        let Some(dcs) = so.get("m_drawCalls").and_then(Value::as_array) else {
            continue;
        };
        for (dc_i, dc) in dcs.iter().enumerate() {
            if found.is_some() {
                return Err(DecodeError::Model(
                    "replace-in-place needs a mesh part with exactly one draw call (this part \
                     renders more than one material)",
                ));
            }
            found = Some((so_i, dc_i, material_of(dc).to_owned()));
        }
    }
    found.ok_or(DecodeError::Model("target mesh part has no draw call"))
}

/// Appends a `(path, new)` scalar edit only when it differs from the current
/// value. A KV3 `0`/`1` is a *tagless* constant with no byte to overwrite, so a
/// no-op set (e.g. the first layout field's offset, always 0) must be skipped or
/// `set_scalars` would fail to resolve it.
fn push_if_changed(
    edits: &mut Vec<(Vec<Seg>, i64)>,
    path: Vec<Seg>,
    current: usize,
    new: usize,
) -> Result<(), DecodeError> {
    if current != new {
        edits.push((path, as_i64(new)?));
    }
    Ok(())
}

/// Reads an unsigned draw-call field's current value (for the changed-or-skip
/// comparison); a sentinel forces a set on the (never-seen) absent field.
fn dc_uint(dc: &Value, key: &str) -> usize {
    dc.get(key)
        .and_then(Value::as_int)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(usize::MAX)
}

/// A `Seg::Key` from a string literal (the verbose `Seg::Key(s.into())` is the
/// bulk of a KV3 path).
fn seg(key: &str) -> Seg {
    Seg::Key(key.to_string())
}

/// Extends a KV3 path base with one final object key.
fn child(base: &[Seg], key: &str) -> Vec<Seg> {
    let mut p = base.to_vec();
    p.push(Seg::Key(key.to_string()));
    p
}

/// A count/size/offset as the `i64` `set_scalars` takes, erroring on the
/// (unreachable for real models) overflow rather than wrapping.
fn as_i64(n: usize) -> Result<i64, DecodeError> {
    i64::try_from(n).map_err(|_| DecodeError::Model("count exceeds i64 range"))
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
