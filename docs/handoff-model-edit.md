# Handoff: editing hero models (geometry), not just textures

## The question this answers

"Can vpkmerge give Vindicta different clothes?" The honest answer splits in two,
because *recoloring existing geometry* and *changing geometry* are completely
different problems. Recoloring is done (the texture round-trip, see
[handoff-texture-edit-cli.md](./handoff-texture-edit-cli.md) +
[findings-deadlock-skin-textures.md](./findings-deadlock-skin-textures.md)).
Geometry is not, and this doc scopes what it would take.

## Current state: the model pipeline is one-way

```
.vmdl_c  --morphic::model::decode-->  Model  --to_glb_textured-->  .glb   (done, M1..M7)
.glb     --> edit in Blender                                              (external, fine)
.glb     --> .vmdl_c                                                      DOES NOT EXIST
```

Confirmed by grep across the workspace: there is no `to_vmdl` / `encode_model` /
`encode_vertex_buffer` / `encode_index_buffer` anywhere. `morphic/src/meshopt/`
ships only `decode_*`. The morphic README's encoder milestones (E1..E3) are
entirely about *textures*. The `.glb` is a terminal preview/render output (for the
Grimoire Locker); nothing reads it back into the game.

## Two facts that shape the build (found 2026-05-28)

1. **KV3 v4-uncompressed is engine-accepted, proven in-game.** The soundevents
   spike's Phase 4 passed: the engine loaded our uncompressed-v4 KV3 re-encode and
   a `volume` edit took effect live (see [spike-vsndevts-kv3.md](./spike-vsndevts-kv3.md)).
   So we never need a KV3 v5 / LZ4 *encoder* for model metadata blocks either; the
   v4-uncompressed writer morphic already has should suffice (needs confirming for
   model blocks specifically, but it is a strong prior).
2. **Deadlock's Source 2 tools are Windows-only.** `Deadlock/game/bin/` contains
   only `win64/`; the compiler ships as `resourcecompiler.dll` and the modeldoc
   editor assets live under `game/core/tools/`, but there are no Linux tool
   executables. On Linux the official ModelDoc/resourcecompiler route means
   Proton/wine or a Windows box, it is GUI-heavy, and vpkmerge cannot automate it.

## Tier the work: displacement vs topology

| Tier | Edit | What changes in the `.vmdl_c` | Difficulty | Path |
|---|---|---|---|---|
| **0 - displacement** | Reshape/sculpt existing mesh: bulk out the dress, alter silhouette, move verts | Only the **MVTX vertex bytes**. Vertex/index counts, index buffer, layout, draw calls, materials, bone bindings all unchanged | **Same class as the texture splice** | Pure-Rust splice in morphic |
| **1 - topology** | *Add* new clothes mesh, accessories, swap body parts | Vertex/index **counts**, attribute layout, draw calls, AABB, skin weights -> full container rebuild | Large | Official tools (Win/Proton) **or** a heavy Rust container writer |

Tier 0 is the model-world analog of `replace_mip_chain`: same container, same
metadata, swap only the payload bytes and splice into the original envelope. Tier 1
means rebuilding the whole Source 2 resource with self-consistent
counts/layout/draw-calls/weights, which is what `resourcecompiler` exists to do.

## Recommendation

**Build Tier 0. Defer Tier 1.** Tier 0 is pure-Rust, cross-platform, ships in
vpkmerge, reuses the splice pattern we have already proven for textures, and covers
a real chunk of "different look" (reshape/bulk/silhouette). Do not commit to either
the Windows toolchain or a full Rust container writer for Tier 1 until Tier 0 has
taught us how strictly the engine validates a re-encoded model block. If real new
geometry is later needed, the honest path is ModelDoc on Windows/Proton, which is a
"what hardware do you have" decision, not a vpkmerge feature.

## Tier 0 build plan (what to write, what already exists)

Already in place (reuse, do not rebuild):

- `morphic::resource::rebuild_with_data` rebuilds the container with one block
  swapped and recomputes the block table. The mechanism (resolve every block's
  payload, swap one, recompute the table + offsets) is generic; generalize it to
  swap an arbitrary block by index, not just `DATA`.
- `morphic::model::vbib` parses the full vertex layout: `BufferDesc` (block index,
  `element_count`, `element_size` = stride) and per-attribute `fields`
  (`semantic_name`, `offset`, format). `decode()` meshopt-decodes the MVTX block to
  the interleaved `vertex_count * stride` byte stream; `positions()` reads POSITION
  out of it. The read side of interleave/deinterleave is done.
- `morphic::meshopt::decode_vertex_buffer` (codec v1, validated byte-exact vs VRF).
- `vpkmerge_core::pack` gets the spliced `.vmdl_c` into an addon VPK (same primitive
  texture-edit and soundevents use). Packaging is solved.

New work, smallest first:

1. **`morphic::meshopt::encode_vertex_buffer(count, stride, &interleaved)` - DONE
   (branch `feat/model-vertex-encode`).** The one genuinely new primitive, the
   inverse of the decoder (codec v1, header `0xa1`). Implemented correctness-first:
   every byte lane is a literal (control nibble `3`) carrying a byte-wise
   zigzag-delta residual (channel `0`). Not size-optimal (output is ~uncompressed
   stream + small per-block control overhead), but it round-trips exactly through
   the VRF-matched decoder the engine also uses, which is all the splice needs.
   - **Offline gate (no game): PASSED.** `meshopt::tests::vertex_encode_round_trips_through_decoder`
     decodes each committed hornet vertex fixture (strides 60/56/52, 598/2643/2495
     verts, multi-block), re-encodes, re-decodes, and asserts byte-identity *and*
     the oracle SHA-256. Plus `zigzag8_inverts_unzigzag8` (exhaustive 0..=255).
   - Note: no index encoder yet (`encode_index_buffer`); displacement keeps indices,
     so it is not needed until Tier 1.
2. **Vertex-stream write path - DONE.** `OnDiskBuffer::write_positions` (in `vbib`)
   overwrites the POSITION lane in place, leaving every other attribute byte-identical,
   and rejects a count change (topology guard). Unit-tested
   (`write_positions_touches_only_the_position_lane`).
3. **`morphic::resource::rebuild_with_block(index, new_bytes)` - DONE.** Generalized
   from `rebuild_with_data` (which now delegates to it); swaps a block positionally,
   preserving block order/count so `CTRL` references stay valid. Unit-tested with a
   synthetic container (`rebuild_with_block_swaps_one_and_preserves_others`).
4. **`morphic::model::replace_vertex_positions(vmdl_bytes, block_index, &positions)`
   - DONE** (in `model/edit.rs`), plus `vertex_targets` (enumerate editable buffers)
   and `read_vertex_positions` (read current). The public splice: parse CTRL, find
   the buffer's `BufferDesc` by block index, decode MVTX, `write_positions`,
   `encode_vertex_buffer`, `rebuild_with_block`. **End-to-end gate PASSED** on the real
   hornet model (`model_local::displacement_edit_round_trips_local`, gated on
   `MORPHIC_MODEL_VPK`): translates one 56,899-vertex buffer, re-decodes, and asserts
   exactly that buffer's positions shifted by the delta with all normals/uv/joints/
   weights/indices byte-identical.
5. **CLI/core glue - DONE.** `vpkmerge model edit --vpk <vpk> --entry <path> [--base]
   [--list] [--part <name>] [--scale S] [--translate x,y,z] --encode-vpk OUT [--vpk-entry]`,
   modeled on `soundevents --encode-vpk`. Core: `vpkmerge_core::edit_model_geometry`
   (+ `model_vertex_targets`, `GeometryEdit`/`GeometryEditReport`) reads the model from
   a VPK, applies a centroid-scale + translate transform to the selected editable
   buffers, splices via morphic, and `pack`s a standalone addon VPK. `--list`
   enumerates buffers (mesh part / block / vertex count / editability).
   Verified producing a loadable addon VPK from the local pak (gun-scale and
   full-body-translate edits both re-decode as valid models).

**Tier 0 is DONE, in-game confirmed (2026-05-28).** A `vpkmerge model edit
--part gun --scale 2.5` addon (`hornet`) loaded in Deadlock and rendered the
oversized gun correctly: the engine accepts our pure-Rust meshopt re-encode and
spliced `MVTX`, with no crash or mesh corruption. So a re-encoded vertex buffer is
engine-valid (the crux risk is retired). The only thing between this and editing
geometry freely is the arbitrary-Blender-reshape path (see risk below); the
built-in transform path is complete.

## Tier 0's real correctness risk: vertex-order mapping

The splice requires the edited positions to come back in **the same vertex order
and count** as the original MVTX.

The **built-in transform path (what shipped) is immune**: it reads positions,
transforms them, and writes them back in native order without ever leaving Rust,
so order and count are trivially preserved. The `write_positions` count guard
rejects any mismatch (the dimension-mismatch analog of the texture path).

The risk bites only the **future arbitrary-reshape path** (import an edited mesh
from Blender): morphic controls the glTF vertex order (we write the `.glb`), so a
*displacement-only* Blender edit (move verts, no add/remove/weld, no re-import that
reorders) maps back identity, but a weld/reorder would break it. That path needs a
stable vertex-id channel to remap on the way back. Tier 0 stays **position
displacement with topology preserved**; anything that changes vertex count or order
is Tier 1.

Open sub-questions to settle during the spike:
- Does Blender's glTF import/export preserve vertex order on a position-only edit,
  or does it weld/reorder? If it reorders, we need a stable vertex-id channel
  (export a custom attribute or vertex index) to remap on the way back.
- Does the engine recompute normals/tangents, or do moved positions need their
  NORMAL/TANGENT recomputed too (else shading breaks on the reshaped area)? If the
  latter, recompute them in the write path from the new positions + topology.
- Are there other blocks that cache geometry-derived data keyed to positions (AABB
  in `DATA`, `PHYS` collision, `DSTF` cloth)? AABB drift is cosmetic-ish; `PHYS`
  is unused for visual mods. Note what we skip.

## Done when (Tier 0) - ALL MET (2026-05-28)

`vpkmerge model edit` takes a transform, splices the new POSITION stream into the
original `.vmdl_c`, and packs an addon VPK that loads in Deadlock showing the
reshaped geometry. Offline gates (met): meshopt encode identity round-trip, and
`model::decode` of the spliced output yields the edited positions with all other
attributes unchanged, green under `cargo test --workspace` with
`clippy --all-targets -D warnings` + `fmt --check` clean. In-game gate (met):
a `hornet` gun-scale addon rendered correctly in Deadlock.

## Tier 0.5 - Blender reshape round-trip: DONE (branch `feat/model-blender-reshape`)

Arbitrary in-Blender reshape of existing geometry (not just math transforms),
topology preserved. Shipped as `vpkmerge model edit --export-glb` / `--from-glb`
/ `--block`, built on `morphic::model::{export_buffer_for_edit, apply_edited_glb}`
+ `glb::to_edit_glb` (a `_ORIGID` per-vertex carrier). Validated end to end on the
real hornet gun via Blender MCP: export -> import (carrier kept as POINT FLOAT
`_ORIGID`) -> non-uniform 2.5x stretch -> export (Blender split 11750->16088 verts)
-> `apply_edited_glb` regrouped split copies by id back to 11750, spliced, and the
gun's long axis measured **68.38 -> 170.94 = 2.50x** in the re-encoded model, other
axes 1.00x. CI gates green; gated identity round-trip green. **In-game visual
confirm of the gun stretch (addon `pak82`) was still pending at handoff.** Commit
`04b2d06`; not yet pushed/PR'd.

Tier 0.5 limits (still topology-preserving): keeps original normals/tangents (heavy
reshapes get stale shading until a normal *encoder* lands); per single buffer;
`apply_edited_glb` rejects added/removed vertices by design.

## Tier 1 - add / remove geometry (new clothes): HANDOFF

Goal: remove a garment and/or add a new one. This is topology change (vertex/index
counts, draw calls, materials, skin weights all differ), so the `_ORIGID`
displacement path does NOT apply. Recommended approach: a **pure-Rust
glTF->`.vmdl_c` writer**, built smallest-first. (Alternative: official
ModelDoc/resourcecompiler, but that is Windows-only in the Deadlock install and not
automatable.)

### The riskiest unknown, and how to retire it first
Tier 0/0.5 only ever re-encoded the **MVTX** block; we have **never had the engine
load a re-encoded model KV3 block** (MDAT/DATA/CTRL). KV3 v4-uncompressed is engine
-accepted for *soundevents* (spike Phase 4), but NOT yet confirmed for *model*
blocks. So:

- **T1a - REMOVE a draw call (cheapest, validates the KV3 risk).** Decode MDAT
  (`m_sceneObjects[].m_drawCalls`), drop the draw call whose `m_material` is the
  target garment, re-encode MDAT with `morphic::encode_kv3_resource` (v4
  uncompressed), splice with `resource::rebuild_with_block`, pack. No new encoders
  needed - reuses the KV3 writer + block rebuild we already have. If this loads
  in-game (the garment disappears), the model-KV3 rewrite path is proven and
  everything below is unblocked. **Do this first.**

### Then build, in order (T1b -> T1d)
- **T1b - meshopt index encoder.** `meshopt::encode_index_buffer` (inverse of the
  existing `decode_index_buffer`; codec v1). Gate: `decode(encode(x)) == x` on the
  committed `*_i*.meshopt` fixtures, mirroring the vertex-encoder test.
- **T1c - new vertex buffer assembly + skin-weight encode.** Read the new mesh from
  an edited glb (the `gltf` reader gives positions, normals, uv, and
  `read_joints`/`read_weights`). Build a fresh interleaved vertex stream at a chosen
  layout - simplest: POSITION `R32G32B32_FLOAT`, NORMAL `R32G32B32_FLOAT`
  (uncompressed; confirm the engine accepts float normals - VRF reads them, so
  likely yes, else add a packed-normal encoder), TEXCOORD `R32G32_FLOAT`,
  BLENDINDICES `R8G8B8A8_UINT`, BLENDWEIGHT `R8G8B8A8_UNORM`. Map glb JOINTS_0
  (skin-joint = model-bone order) back through the mesh **bone remap table**
  (`skeleton::remap_table`, inverted) to mesh-local indices. meshopt-encode the new
  MVTX + MIDX.
- **T1d - rewrite the KV3 metadata + container.** Update CTRL `embedded_meshes`
  buffer registry (`m_nElementCount`, `m_nElementSizeInBytes`, `m_nBlockIndex`,
  `m_inputLayoutFields` for the new layout), MDAT draw calls (new counts / start
  index / base vertex / material), DATA LOD masks + bounds, and RERL (add the new
  `.vmat_c` to external refs). Re-encode each KV3 (v4), then rebuild the container
  swapping MVTX+MIDX+MDAT (and adding blocks if vertex/index buffers grow) - this
  needs a **multi-block / add-block** generalization of `rebuild_with_block` (it
  currently swaps one same-or-different-size block in place). Package the new
  `.vmat_c` + textures into the addon VPK alongside the model (reuse `pack`).

### Reuse (already built)
meshopt **vertex** encoder; KV3 v4 writer (`encode_kv3_resource`, engine-accepted
for soundevents); `resource::rebuild_with_block`; the `gltf` reader (positions/
normals/uv/joints/weights); `vbib` layout parsing; `pack`. The glb edit-export
(Tier 0.5) is the artist's starting point for the new mesh.

### Scope notes
- "Replace one mesh part wholesale" (new dress buffer of any vertex count + its
  draw calls + material) is the cleanest first *add*, after T1a proves remove.
- New geometry must be **weight-painted to the skeleton in Blender** or it won't
  move with the hero - that is artist work plus T1c's weight encode.
- New clothes need LOD0 only; drop/duplicate other LODs for the swapped part.
- A genuinely new material (T1d RERL + texture packaging) can be deferred by reusing
  an existing hero material for the new mesh at first.
