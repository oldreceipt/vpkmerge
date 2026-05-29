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

### The riskiest unknown - RETIRED in-game (2026-05-28)
Tier 0/0.5 only ever re-encoded the **MVTX** block; we had **never had the engine
load a re-encoded model KV3 block** (MDAT/DATA/CTRL). **T1a retired this:** the
engine loads an edited model `MDAT` block and renders the result, with no ERROR.
Two hard-won facts shaped the working implementation:

1. **The lossy `Value`-tree KV3 re-encode does NOT work for model blocks.** Our
   `kv3::encode` (v4 uncompressed) faithfully round-trips at the `Value` level and
   VRF reads its output, but the *engine* substitutes the ERROR placeholder model.
   Cause: the writer drops KV3 **value flags** (model `MDAT` carries the `resource`
   flag on `m_material`, 6 of them) and flattens **auxiliary-buffer typed arrays**
   (`MDAT` has ~499; node type 25) to generic arrays. soundevents had neither, which
   is why the same writer was fine there (spike Phase 4) but not here. Confirmed
   in-game: an *identity* re-encode (no edit) via the lossy writer also ERROR'd.
2. **A byte-faithful re-wrap loads.** Decompressing the original block's buffers and
   re-emitting them verbatim, only **uncompressed** (`compressionMethod = 0`, same
   v5 layout), preserves every flag/typed-array byte. The engine loads + renders
   that (confirmed: identity re-wrap of all 10 MDAT blocks loaded normally in-game).

So removal does **not** re-encode from the tree. The shipped path:
- `kv3::rewrap_uncompressed` - decompress + re-emit the block uncompressed, byte-faithful.
- `kv3::neutralize_draw_calls` - on the re-wrapped (uncompressed) block, walk the KV3
  tree tracking absolute byte offsets and **zero the target draw calls'
  `m_nIndexCount` in place** (a 0-index draw submits no primitives). Only those few
  ints change; flags/typed-arrays/structure are untouched.
- `model::remove_draw_calls_by_material` - find matching draw calls per `MDAT`
  (all LODs), neutralize, splice with `resource::rebuild_with_block`, return a report.
- core `remove_model_material` / `model_draw_call_targets`; CLI `vpkmerge model edit
  --list-drawcalls` and `--remove-material <NEEDLE> --encode-vpk OUT`. (Diagnostic
  `--reencode-mdat` does the identity re-wrap, used to split the failure above.)

Gates:
- **Offline (CI): PASSED.** `mdat_rewrap_uncompressed_is_value_faithful` (re-wrap ==
  same tree, uncompressed), `neutralizing_dress_zeros_only_its_index_count` (only the
  dress `m_nIndexCount` -> 0, every other byte identical), `find_dress_draw_call_locates_it`.
- **Gated full-model: PASSED.** `tests/model_local.rs::remove_material_round_trips_local`
  neutralizes all 4 dress draw calls (body LOD0-3), total indices 426927 -> 331290,
  every other primitive's indices and all vertex buffers byte-identical.
- **In-game: PASSED (addon `pak92`).** Vindicta loads and renders, dress gone, no
  ERROR. So the engine (a) accepts our byte-faithful model-KV3 edit and (b) tolerates
  a 0-index draw call. The model-KV3 rewrite path is proven; T1b->T1d unblocked.

Caveats / notes:
- VRF's *glTF exporter* throws on a 0-index draw call (glTF forbids zero-length
  accessors); that is an export-format rule, not a load rule - VRF's loader and the
  game both accept it. So the addon can't be re-exported to glb, only loaded.
- Neutralize leaves the draw call in place (count 0) and its now-unreferenced geometry
  in the buffers (dead bytes, not drawn). A *true* structural delete (shrinking the
  `m_drawCalls` array) would need a faithful KV3 structural editor (recompute lane
  counts/sizes); not built - unnecessary for hiding a part.
- **Content fact:** `vindicta_dress.vmat`'s draw call is the dress fabric **and** the
  body/torso skin fused as one draw call (45029 verts / 95637 idx). Draw-call removal
  is all-or-nothing per material, so removing it drops the torso too. Removing *only*
  the fabric while keeping the body is not possible via draw-call removal (the game's
  model does not separate them); it needs mesh surgery or the texture route.

### Then build, in order (T1b -> T1d)
- **T1b - meshopt index encoder: DONE.** `meshopt::encode_index_buffer` (inverse of
  the existing `decode_index_buffer`; codec v1, header `0xe1`). Correctness-first,
  mirroring the vertex encoder's philosophy: every triangle uses the fully-explicit
  code (`codetri = 0xff`, `codeaux = 0xff`), so all three indices are zigzag-vbyte
  deltas in the data stream and read back without depending on the edge/vertex FIFO
  history. Not byte-identical to Valve's compressor (forgoes the FIFO-relative
  codes), only round-trip equivalent under the same VRF-matched decoder the engine
  uses. **Offline gate (no game): PASSED.**
  `meshopt::tests::index_encode_round_trips_through_decoder` re-encodes each committed
  index fixture's decoded triangle list (gun_lod3 11040 idx, body_lod3 1530 idx, both
  u16), re-decodes, and asserts byte-identity *and* the oracle SHA-256; plus
  `index_encode_round_trips_u32` covers the 32-bit lane (the committed fixtures are all
  u16) with a synthetic list including a backwards jump. CI green (fmt/clippy/test).
  In-game proof of an index re-encode is deferred to the T1c/T1d add-geometry round-trip.
- **T1c - new vertex buffer assembly + skin-weight encode: DONE.** Reads a new mesh
  part from an edited glb and encodes it to `MVTX` + `MIDX`, ready for T1d to splice.
  Shipped pieces:
  - `model::read_edited_mesh(glb, mesh_name)` (in `edit.rs`) - extends the glb reader
    beyond positions to normals, UV0, `JOINTS_0`, `WEIGHTS_0`, and the index list, via
    the `gltf` reader. Takes one primitive (the add-one-part contract; `mesh_name`
    picks it out of a multi-mesh glb). Positions in accessor space (T1d reconciles any
    axis flip).
  - `model::assemble_vertex_buffer(&VertexBuffer)` (in `mesh.rs`, the inverse of
    `deinterleave`) -> interleaved stream at a fixed uncompressed layout: POSITION
    `R32G32B32_FLOAT`, NORMAL `R32G32B32_FLOAT`, TEXCOORD `R32G32_FLOAT`, BLENDINDICES
    `R8G8B8A8_UINT`, BLENDWEIGHT `R8G8B8A8_UNORM` (40-byte skinned stride; optional
    attrs omitted if absent). Every format is already decode-supported, so the output
    reads straight back through the model path. Weights quantized to `u8`-unorm summing
    to 255 (`quantize_weights_u8`).
  - `model::build_mesh_buffers` / `build_mesh_buffers_from_glb` -> `EncodedMesh`
    (`mvtx`, `midx`, vertex/index counts, stride, index width, layout `fields`) - the
    bundle T1d registers in the container.
  - **Identity-remap first cut:** `BLENDINDICES` are written as the glb `JOINTS_0`
    values verbatim (model bone indices, must be `<= 255`; the 62-bone hornet fits).
    Inverting `skeleton::remap_table` to compact mesh-local indices is **deferred to
    T1d** (which writes the new mesh's `m_remappingTable`); an identity remap there
    means BLENDINDICES = model bone index directly.
  - **Open question for T1d/in-game:** does the engine accept uncompressed float
    `NORMAL` (`R32G32B32_FLOAT`)? VRF reads it, so likely yes; if not, add a
    packed-normal (`R32_UINT` v2) encoder. Untested until a real add loads in-game.
  - **Gates (offline, CI green):** `mesh::assemble_tests::assemble_round_trips_through_encode_and_deinterleave`
    (synthetic: assemble -> meshopt-encode -> decode -> deinterleave recovers
    positions/normals/uv/joints exactly, weights within 1/255), plus the quantizer +
    over-255 + positions-only cases; `edit::tests::build_mesh_buffers_round_trips_quad`
    (both encoders). **Gated real-data (PASSED):**
    `build_mesh_buffers_round_trips_real_glb_local` on the hornet `to_glb_textured`
    export's gun part (`MORPHIC_EDIT_GLB`) round-trips **11750 verts / 76329 indices**
    (joints+weights+uv+normals all present): positions/normals/uv exact, joints exact,
    weights within 3/255, the full triangle list exact through the T1b index encoder.
- **T1d - rewrite the KV3 metadata + container.** Splice the T1c-encoded buffers
  into a loadable model. Scoped 2026-05-28 against the code; the route changed from
  the original "uncompressed layout + array growth" guess.

  **The two hard walls (both confirmed in code):**
  1. *KV3 array growth.* `kv3::rewrap_uncompressed` is byte-faithful (preserves the
     value flags + node-type-25 typed arrays the engine demands) and the `kv3::patch`
     walker tracks absolute offsets, but it only *locates/edits scalars* - there is no
     logic to insert an array element (a new draw call / buffer-registry entry / remap
     entry), which means splicing bytes into four typed lanes at once, fixing counts,
     re-aligning downstream lanes, and rewriting the v5 object-length table. The lossy
     `kv3::encode` writer stays ruled out (T1a). 
  2. *Container can't add blocks.* `resource::rebuild_with_block` only *swaps* a block
     (size changes fine), preserving block count/order so existing CTRL `m_nBlockIndex`
     refs stay valid. Adding a new `MVTX`/`MIDX` block needs a generalization that
     grows the block table **and rewrites every `m_nBlockIndex`**. And **RERL has no
     parser/writer** (a genuinely new `.vmat_c` ref would need one).

  **The wedge that sidesteps both: replace one mesh part *in place*.** Instead of
  *adding* a separate part, **replace an existing part's geometry** (new mesh, any
  vertex/index count). That needs only block *swaps* + in-place *scalar* edits - the
  exact mechanism T1a proved in-game:
  - Swap the part's `MVTX`+`MIDX` (reuse its block indices -> no table growth, no
    `m_nBlockIndex` rewrite); reuse its material (no RERL change).
  - Keep `m_inputLayoutFields`' *element count* unchanged by assembling the new buffer
    at the target's exact field *set* (same semantics) but uncompressed formats
    (T1d-b); then only each field's `m_Format`/`m_nOffset`, the stride, the buffer
    `m_nElementCount`s, and the draw call's `m_nVertexCount`/`m_nIndexCount`/
    `m_nStartIndex`/`m_nBaseVertex` change - all **scalars**.
  - Conform to the target mesh's existing bone remap (invert it, T1d-c) so the remap
    table needs no edit.

  Wedge sub-steps:
  - **T1d-a - scalar-set patch primitive: DONE.** `kv3::set_scalars(block, &[(path,
    value)])` (in `kv3/patch.rs`): a path-tracking sibling of the neutralize walker
    (shares the extracted `lanes()` layout) that locates integer scalars by KV3 path
    and sets them in place on a `rewrap_uncompressed` block, erroring if a value does
    not fit the field's existing on-disk width (no width change = no structural
    re-encode). **Offline gate (CI green):** `model::tests::set_scalars_edits_field_by_path`
    sets the dress `m_nIndexCount` by path and asserts the output is **byte-identical to
    `neutralize_draw_calls`** (cross-checks the new walker against the proven one), then
    sets a new value and confirms only that field changed; plus a missing-path reject.
  - **T1d-b - assemble at a target field set.** Extend T1c assembly to emit a *given*
    field set at uncompressed formats (derive TANGENT, synthesize any missing semantic)
    so the layout's field count is preserved.
  - **T1d-c - invert the target mesh's `skeleton::remap_table`** (local<-model) and map
    the new mesh's model-bone `JOINTS_0` into that mesh's local `BLENDINDICES` space.
  - **T1d-d - orchestrate + in-game gate:** swap blocks, `set_scalars` the counts /
    formats / offsets / stride, `pack` an addon VPK. The in-game load also **answers the
    float-`NORMAL` question** (the wedge upgrades the target's compressed normals to
    `R32G32B32_FLOAT`).

  **Deferred to "true additive" (keep the body *and* add a new garment):** the KV3
  array-growth splice (wall 1), the container add-block + `m_nBlockIndex` rewrite
  (wall 2), and a RERL writer for a brand-new material (reuse a hero material first).

### Reuse (already built)
meshopt **vertex** encoder + **index** encoder (T1b); the T1c **mesh assembler**
(`model::{read_edited_mesh, assemble_vertex_buffer, build_mesh_buffers}` ->
`EncodedMesh`); `resource::rebuild_with_block`; `kv3::rewrap_uncompressed`
(byte-faithful uncompressed re-emit, engine-accepted for model blocks) +
`kv3::patch`/`neutralize_draw_calls` (in-place scalar *zero* with absolute-offset
walking) + `kv3::set_scalars` (T1d-a: in-place scalar *set* by KV3 path, width-safe);
the `gltf` reader (positions/normals/uv/joints/weights); `vbib` layout
parsing; `pack`. The glb edit-export (Tier 0.5) is the artist's starting point for
the new mesh. **Note:** the `kv3::encode` v4 writer is engine-accepted for
*soundevents* but **NOT** for *model* blocks (see T1a); use `rewrap_uncompressed` +
in-place patch for models.

### Scope notes
- "Replace one mesh part wholesale" (new dress buffer of any vertex count + its
  draw calls + material) is the cleanest first *add*, after T1a proves remove.
- New geometry must be **weight-painted to the skeleton in Blender** or it won't
  move with the hero - that is artist work plus T1c's weight encode.
- New clothes need LOD0 only; drop/duplicate other LODs for the swapped part.
- A genuinely new material (T1d RERL + texture packaging) can be deferred by reusing
  an existing hero material for the new mesh at first.
