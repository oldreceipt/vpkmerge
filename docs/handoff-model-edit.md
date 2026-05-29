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

**Remaining for Tier 0: in-game confirmation only** (the offline path is complete).
Drop the addon VPK in `citadel/addons/` and verify the reshape renders. The one open
risk below (arbitrary Blender reshape vs. the built-in transform) is what stands
between this and editing geometry freely; the transform path needs no Blender and is
ready to test in-game now.

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

## Done when (Tier 0)

`vpkmerge model edit` takes an edited mesh (or edited-positions input), splices the
new POSITION stream into the original `.vmdl_c`, and packs an addon VPK that loads
in Deadlock showing the reshaped geometry. Offline gates: meshopt encode identity
round-trip, and `model::decode` of the spliced output yields the edited positions
with all other attributes unchanged, both green under `cargo test --workspace` with
`clippy --all-targets -D warnings` + `fmt --check` clean. Then in-game confirm on a
Vindicta (`hornet`) reshape, mirroring the texture round-trip's in-game check.

## Out of scope (Tier 1, note for later)

- Adding/removing geometry, new garment meshes, accessories: needs a full S2
  container writer (all blocks re-emitted with consistent metadata) or the official
  ModelDoc + resourcecompiler toolchain (Windows/Proton). Decide custom-encoder vs
  official-tools only after Tier 0, informed by how the engine validates re-encoded
  blocks and by the user's Windows availability.
- Index-buffer editing (`encode_index_buffer`): not needed for displacement; part
  of Tier 1.
