# `.vmdl_c -> .glb` exporter: progress + continuation brief

Live handoff state for the model exporter work. Status: **M1..M7 COMPLETE**. The
exporter is done: `vpkmerge model export` turns a `.vmdl_c` (base or skin VPK)
into a textured, skinned, **animated** `.glb` (the hero's own clips, by default
all of them; `--clip <name>` / `--no-anim` narrow it). Remaining work is
Grimoire-side only (wire the IPC that runs the exporter on an installed skin +
load the `.glb` in the Locker; now that each hero GLB carries its own clips,
drop the cross-hero retarget assumption in `HeroModelViewer.tsx`). Scope
authority is `vmdl-glb-exporter-handoff.md` (+ `vmdl-glb-exporter.md`); this file
tracks what is built, what was learned, and how to continue. You do NOT touch
the Grimoire/Electron side.

## Working cadence (user's choice)

Checkpoint per milestone: implement, get green against the VRF oracle, commit,
then STOP and report and wait for greenlight before the next milestone. Work on
branch `feat/vmdl-glb-export`; commit per green milestone. Hard rule: **no
em-dashes anywhere** (code, comments, commit messages, replies).

## Status

| Commit | Milestone | What |
|---|---|---|
| `d8f9b9f` | M0 | vmdl_c inspect (pre-existing) + handoff docs. |
| `aed2240` | Oracle | `model` (golden .glb via VRF GltfModelExporter, animations ON so skeleton/skin is present) + `kv3-dump`; committed hornet KV3 fixtures. |
| `0efe633` | M1 | KV3 v5 binary parser (`morphic/src/kv3/`). Validated byte-exact vs oracle. |
| `2e9c0c3` | M2 | Pure-Rust meshopt vertex+index decoders (`morphic/src/meshopt/`). Validated byte-exact vs VRF. |
| `d7cb3ba` | M3 | Mesh assembly + skeleton + skin (`morphic/src/model/`): in-memory `Model`. Validated vs oracle `model-meta` golden. |
| `70e97d8` | M4 | Material (`.vmat_c`) parsing (`morphic/src/material/`): shader + param tables + name-based PBR slots. Validated vs oracle `material-meta` golden. |
| `18d2f39` | M5a | GLB writer (`morphic/src/model/glb.rs`): geometry + skeleton + skin, untextured. Round-trips through the `gltf` reader. |
| `47ef9ef` | M5b | Textured GLB (`to_glb_textured` + `FileResolver`): base color / normal / roughness / occlusion / emissive. Confirmed fully textured in Blender. |
| `9ef786f` | M6 | `model export` CLI + `vpkmerge-core::export_model` (cross-VPK resolver) + refreshed bundled binary. |
| (this) | M7 | Animation (clip) export: `ANIM`/`ASEQ`/`AGRP` decode (`morphic/src/model/animation.rs`) + glTF clip emit (`glb.rs`) + `--clip`/`--no-anim`. Validated vs oracle `anim-meta` golden (276 clips + sampled keyframes). |

Remaining: Grimoire-side integration only (separate repo): an IPC that runs
`vpkmerge model export` on the installed skin VPK and loads the `.glb` in the
Locker. Each hero GLB now ships its own clips (no cross-hero retarget), so
`HeroModelViewer.tsx` should load per-hero clips directly. The bundled
`grimoire/public/models/*.glb` example assets can be re-exported from correct
in-game models with this exporter.

- M6 detail: `vpkmerge model export --vpk <vpk> --entry <path.vmdl_c> [--base
  <pak01_dir.vpk>] --out <file.glb>`. The bare `vpkmerge model <vpk>` inspect
  form is unchanged. `vpkmerge-core::export_model` opens the VPK(s), reads the
  entry, `model::decode` + `model::to_glb_textured` with a `VpkResolver` (skin
  first, base second). The bundled binary
  `../grimoire/resources/vpkmerge/vpkmerge-linux-x86_64` (gitignored) is rebuilt
  with `cargo build --release -p vpkmerge-cli` + copied. Windows: cross-build
  + copy the `.exe` target when packaging for Windows (not done here).
  Validated: exports both the base hornet (from pak01) and the bunnysuit skin
  VPK to fully-textured `.glb` (eyeballed in Blender).

- M7 detail (animation): `morphic/src/model/animation.rs` is a verbatim port of
  VRF `ResourceTypes/ModelAnimation` (`Animation`, `AnimationDataChannel`,
  `AnimationSegmentDecoder`, `Frame`, `SegmentHelpers`, the 10
  `CCompressed*` segment decoders incl. the 6-byte packed quaternion and the
  half-float anim/delta vectors). `model::decode` now fills `Model.animations:
  Vec<Clip>` (best-effort; a model with no `ANIM` block exports statically). The
  decode-key channels map onto the model `DATA` skeleton **by bone name** (the
  AGRP `m_boneArray` is not needed), so a clip drives that hero's own bind pose
  with no retargeting. `glb.rs::add_animations` emits one glTF animation per clip
  (sampler per animated bone+channel, time = `frame/fps`, raw Source local space
  like the bind-pose nodes; `--clip`/`--no-anim` filter in `vpkmerge-core`). The
  expected clip set comes from `ASEQ` (sequence names) plus leftover `ANIM`
  anims, matching VRF's `GltfModelExporter`. Per-hero own-animations replaced the
  dead cross-hero retarget plan (heroes do not share a skeleton: hornet 62 bones,
  bookworm 164). Validated vs oracle `anim-meta`: 276 clips (names + fps +
  frame_count + looping exact) and sampled per-bone keyframes within 2e-3
  (gated `tests/anim_local.rs`); CI smoke test round-trips a synthetic clip.
  `glb_viewer` loads `(skin: true, N animation(s))` and plays the idle.

- M5b detail: `to_glb_textured(&Model, &dyn FileResolver) -> Vec<u8>`. The caller
  implements `FileResolver` (compiled path -> bytes; skin VPK first, base pak
  second), so morphic stays I/O-free. Per material it parses the `.vmat_c` (M4),
  decodes each PBR-slot `.vtex_c` (`morphic::texture`), and embeds PNGs as glTF
  images wired to baseColor (sRGB) / normal / metallicRoughness (roughness from
  the packed normal alpha) / occlusion / emissive. Two Deadlock-specific gotchas
  (found via Blender ground-truth, both fixed): (1) inverted-hull toon-OUTLINE
  primitives (`*_outline`) are dropped, else the shell whitewashes the model;
  (2) `<=4x4` placeholder textures are skipped, else the solid-white default
  self-illum mask makes every surface glow white. Base-color alpha is forced
  opaque for non-blend materials (Source albedo alpha is a mask, not opacity).
  NOT reproduced (runtime NPR, out of scope): the toon outline itself, rim light,
  tint masks. The backdrop is a faithful PBR albedo render, not the in-game NPR
  look; doing the toon pass is a three.js (Grimoire) renderer concern.

- M5a detail: `model::to_glb(&Model) -> Vec<u8>` (new dep `gltf-json`, pure-Rust;
  GLB container framed by hand). Emits POSITION/NORMAL/TANGENT/TEXCOORD_0/
  JOINTS_0/WEIGHTS_0 accessors over one packed BIN buffer, the model skin (bone
  node hierarchy + IBM accessor), and untextured default-PBR materials named by
  draw-call path. Coordinate handling mirrors VRF (positions + bone locals in
  Source space; `TRANSFORMSOURCETOGLTF` on the skeleton wrapper + mesh nodes; IBM
  = Source-space `inverse(globalBind)` from M3). morphic's row-major `Mat4` maps
  to glTF column-major verbatim (`Mat4.m` emitted as-is). Replicates VRF's
  default-weight spread (hornet `gun`: joints, no weights) + `FixDuplicateJoints`.
  NOT done (M5b): texture images. Materials reference no textures yet; the GLB
  renders untextured (white default PBR). `FixZeroLengthVectors` for degenerate
  normals/tangents is also deferred (the `gltf` reader accepts the current output).
- M5a validation gap: the writer is structurally validated (committed synthetic
  round-trip + gated hornet round-trip through the `gltf` reader: 3 meshes / 7
  prims / 62 named joints / valid skin). The "looks right + skins + accepts
  `hornet_idle.glb` clips" check needs a maintainer + a viewer/Grimoire (no GUI
  here). Regenerate the file: `MORPHIC_MODEL_VPK=<pak01_dir.vpk> cargo test -p
  morphic --test model_local -- --nocapture` (writes `/tmp/hornet.glb`, override
  with `MORPHIC_GLB_OUT`).

- M4 detail: `material::parse(&[u8]) -> Material` reads a compiled material's
  `DATA` KV3 (shader name + `m_intParams`/`m_floatParams`/`m_vectorParams`/
  `m_textureParams`), mirroring VRF `Material.Read`. `Material::pbr()` is a
  best-effort, NAME-based slot map (`g_tColor`->base color,
  `g_tNormal`/`g_tNormalRoughness`->normal, `g_tAmbientOcclusion`->occlusion,
  `g_tSelfIllumMask`->emissive, plus roughness/metalness when a dedicated slot
  exists); `alpha_mode()` from `F_TRANSLUCENT`/`F_ALPHA_TEST`. Pure: no texture
  resolution/decode here. NOT done in M4 (deferred to M5/M6, where the loader +
  GLB writer live): VRF's shader-aware channel split + ORM packing, resolving
  `.vtex` paths across VPKs, decoding (via `morphic::decode`), and embedding as
  glTF images. The hornet draw-call materials are `pbr.vfx` with 7 texture slots.

- M3 detail: `model::decode(&[u8]) -> Model` reads the 62-bone model skeleton
  (`skeleton.rs`, from `DATA m_modelSkeleton`; local bind = `fromQuat(rot) *
  translate(pos)`, scale ignored per VRF; global bind chained, inverse-bind via
  `math.rs`), the LOD0 meshes (`mesh.rs`: CTRL embedded-mesh registry, LOD filter
  via `DATA m_refLODGroupMasks`, MDAT draw calls -> primitives, MVTX/MIDX via the
  M2 codecs), and deinterleaves attributes per the CTRL input layout (`vbib.rs` +
  `dxgi.rs`, ports of VRF `VBIB.cs`: position/uv/normal-tangent incl. both
  compressed encodings, blend indices + remap, blend weights). Joints are remapped
  to model bone indices via `DATA m_remappingTable[Starts]`. Materials/textures
  are NOT resolved yet (M4); no `.glb` written yet (M5). Structural readers parse
  bone/layout/draw-call structure from KV3 alone, so committed `model::tests`
  validate them without the multi-MB buffers; the gated `tests/model_local.rs`
  decodes the real hornet end to end against the same golden.
- M3 numbers (hornet LOD0, vs oracle): 62 bones, 3 meshes (body/gun/ghost_glow),
  7 primitives, 78111 unique vertices (248808 summed per-primitive, the figure
  the old brief quoted), 426927 indices, 7 materials. `gun` carries BLENDINDICES
  but no BLENDWEIGHT (VRF defaults weights at GLB-write time; replicate in M5).

- M1 detail: ported from VRF `BinaryKV3.cs` KV3_V5 path; `parse()` is
  crate-private. LZ4 via `lz4_flex`. v1-v4 return errors (not needed: every
  hornet KV3 block is v5). ZSTD (via pure-Rust `ruzstd`) + the binary-blob
  section (chained-LZ4 or single ZSTD frame) were added in M7: the `ANIM` block
  is ZSTD and carries its segment buffers as KV3 binary blobs.
- M2 detail: `decode_vertex_buffer` / `decode_index_buffer`, scalar ports of
  zeux/VRF. Pure-Rust on purpose (no C toolchain). Oracle `mesh-buffers` added.

## CRITICAL structural facts (the handoff doc is wrong about buffer descriptors)

The handoff says vertex/index count, stride, and layout come from the MDAT KV3.
They do NOT. Reality:

- The **`CTRL` block is the buffer registry**. Its root has `embedded_meshes[]`
  (10 for hornet). VERIFIED order (the earlier guess was wrong): `body`, `gun`,
  `ghost_glow`, `body_lod1`, `gun_lod1`, `ghost_glow_lod1`, `body_lod2`,
  `gun_lod2`, `body_lod3`, `gun_lod3`. `m_refLODGroupMasks` (in `DATA`, same
  order) = `[1,1,1,2,2,2,4,4,8,8]`, so LOD0 = `mask & 1` = indices {0,1,2} =
  body/gun/ghost_glow (3 meshes, matching the golden). Each entry: `m_Name`,
  `m_nMeshIndex`, `m_nDataBlock` (the MDAT block index), `m_vertexBuffers[]`,
  `m_indexBuffers[]`. Each buffer
  descriptor: `m_nElementCount`, `m_nElementSizeInBytes` (vertex stride, or index
  size 2|4), `m_inputLayoutFields[]` (`m_pSemanticName`, `m_nSemanticIndex`,
  `m_Format` = DXGI_FORMAT int, `m_nOffset`), `m_nBlockIndex` (the MVTX/MIDX
  block, by global block order), `m_bMeshoptCompressed`, `m_bCompressedZSTD`.
- The **MDAT block** (one per embedded mesh) holds
  `m_sceneObjects[].m_drawCalls[]` (each draw call: `m_nVertexCount`,
  `m_nIndexCount`, `m_nStartIndex`, `m_nBaseVertex`, `m_material`,
  `m_nPrimitiveType`, and `m_vertexBuffers[].m_hBuffer` / `m_indexBuffer.m_hBuffer`
  = index into that mesh's CTRL buffer arrays) AND `m_skeleton` (bones, bind
  pose).
- LOD filtering: Model DATA block `m_refLODGroupMasks`. The golden GLB is LOD0.

VRF wiring for reference: `Model.GetEmbeddedMeshes()` reads CTRL
`embedded_meshes`, builds `mesh.VBIB = new VBIB(Resource, embeddedMesh)`; draw
calls index `vbib.VertexBuffers[m_hBuffer]` / `IndexBuffers[...]`.

## Golden GLB targets (hornet, LOD0)

3 meshes / 7 primitives, **248,808 vertices, 426,927 indices (142,309 tris),
7 materials, 25 textures, 62-joint skeleton**. Bone names start: `root_motion`,
`pelvis`, `spine_0..3`, `neck_0`, `head`, `clavicle_L/R`, `arm_upper/lower_L/R`,
`hand_L/R`, `weapon_bone`, `leg_upper_L`, ... prim[0] (body vbuf0) = 56899 verts,
stride 56, attrs POSITION / TEXCOORD / NORMAL / TANGENT / BLENDINDICES /
blendweight.

## M3 to-dos (DONE in `d7cb3ba`)

All landed in `morphic/src/model/`: DXGI subset (`dxgi.rs`), deinterleave +
the `VBIB.cs` helper ports (`vbib.rs`: `GetNormalTangentArray` both compressed
encodings, `GetBlendIndicesArray` + remap, `GetBlendWeightsArray`), draw-call ->
primitive mapping (`mesh.rs`), and the model skeleton with bind / inverse-bind
matrices (`skeleton.rs` + `math.rs`). Validated: joint count + sorted bone-name
set match the golden, and the full decode (positions/normals/joints/totals/bbox)
matches the oracle `model-meta` golden on the real hornet.

Note for M4/M5 (carry forward):
- The skeleton stores `inverse_bind` and local pos/rot per bone; M5 must apply
  the source->glTF axis transform (VRF `TRANSFORMSOURCETOGLTF`) and emit the
  glTF `skin`. VRF also runs `FixZeroLengthVectors` (normals/tangents) and
  `FixDuplicateJoints` at GLB-write time, and defaults weights when a mesh has
  joints but no `BLENDWEIGHT` (e.g. hornet `gun`); morphic defers all three to
  the GLB writer. The body `vb1` carries a `COLOR` attribute morphic currently
  ignores (not in the M3 attribute set).
- Materials (M4): draw call `m_material` paths are already on each `Primitive`.
  Resolve `.vmat_c` (KV3) -> texture params -> `.vtex_c` via the cross-VPK
  loader, decode with the existing `morphic::texture` path.

## Validation method + oracle

- .NET: `~/.dotnet/dotnet` (10.0.300, pinned). Oracle: `cd tools/morphic-oracle
  && ~/.dotnet/dotnet build`, run `bin/Debug/net10.0/morphic-oracle.dll <cmd>` or
  `dotnet run -- <cmd>`. VRF 19.1.6199; API XML at
  `~/.nuget/packages/valveresourceformat/19.1.6199/lib/net10.0/ValveResourceFormat.xml`.
- VRF source: fetch raw from
  `https://raw.githubusercontent.com/ValveResourceFormat/ValveResourceFormat/master/<path>`
  (network works via curl). Useful files: `Resource/ResourceTypes/{BinaryKV3.cs,
  BinaryKV3.NodeType.cs,Mesh.cs,Model.cs}`, `Resource/Blocks/{VBIB.cs,MBUF.cs}`,
  `Compression/MeshOptimizer{Vertex,Index}Decoder.cs`,
  `IO/GltfModelExporter.Mesh.cs`. Find paths via the repo git tree API.
- Oracle subcommands: `model --vpk --entry [--base] --out`,
  `kv3-dump --vpk --entry --block FOURCC [--nth N] --out [--raw]`,
  `mesh-buffers --vpk --entry --out-dir`, `model-meta --vpk --entry --out`
  (compact M3 golden), `material-meta --vpk --entry --out` (M4 golden),
  `anim-meta --vpk --entry --out` (M7 golden: clip list + sampled keyframes).
  Justfile: `just model-golden <entry> <out>`, `just kv3-goldens` (now also dumps
  `CTRL`), `just mesh-buffers`, `just model-meta`, `just material-meta`,
  `just anim-meta`.
- Test data: entry `models/heroes_staging/hornet_v3/hornet.vmdl_c` in
  `~/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`
  (archive parts `_000.._019` present). Regenerate the golden GLB to
  `/tmp/hornet_golden.glb` (large, NOT committed); analyze with a Python
  GLB-JSON-chunk reader. Reference GLBs: `../grimoire/public/models/hornet.glb`
  (mesh) + `hornet_idle.glb` (clips).
- Fixture convention: commit SMALL inputs + golden siblings under
  `morphic/fixtures/<tier>/` (existing `kv3/`, `meshopt/`; keep the dir ~1MB, do
  not commit multi-MB GLBs/buffers). Tests live as `#[cfg(test)] mod tests;`
  INSIDE the private module (kv3, meshopt) because those modules are
  crate-private. For M3, suggest an oracle subcommand that dumps a COMPACT model
  meta golden (per-primitive vertex/index counts, sorted bone-name list,
  material count, bbox) so the test runs in CI without the big GLB.
- Float compare convention (M1): oracle emits floats as `{"$f64":"0xHEXBITS"}`,
  blobs as `{"$bin":{"len","sha256"}}`; Rust compares by bit pattern / sha.

## CI gate (pass before each commit)

`cargo fmt -p morphic -- --check`; `cargo clippy -p morphic --all-targets --
-D warnings` (pedantic is ON); `cargo test -p morphic`. For faithful codec
ports, file-level `#![allow(clippy::cast_possible_truncation,
clippy::too_many_lines, clippy::similar_names, clippy::unusual_byte_groupings,
clippy::needless_range_loop)]` with a rationale comment is the accepted pattern;
fix `is_multiple_of` / `div_ceil` idiomatically. morphic must stay pure-Rust (no
C build deps).

## morphic internals

- `kv3::parse(&[u8]) -> Result<Value, DecodeError>`. `Value` = Null / Bool /
  Int(i64) / UInt(u64) / Double(f64) / String / Binary(Vec<u8>) / Array /
  Object(BTreeMap).
- `meshopt::{decode_vertex_buffer, decode_index_buffer}(count, size, &[u8])`.
- `Resource::parse`, `find_block([u8;4]) -> Option<&[u8]>`, `blocks()`,
  `get_block_by_index(n)` (added in M3 for `m_nDataBlock` / `m_nBlockIndex`).
- `Value` accessors added in M3: `as_str`, `as_array`, `as_object`, `as_uint`,
  `as_f64`, `as_bool`, `get_f64` (alongside the existing `get` / `as_int`).
- `model::decode(&[u8]) -> Model` (M3). `Model { skeleton, meshes }`,
  `MeshPart { vertex_buffers, primitives, .. }`, `VertexBuffer`
  (positions/normals/tangents/texcoords/joints/weights), `Skeleton`/`Bone`.
  Helpers: `total_vertices` (unique), `gltf_vertex_total` (per-primitive sum),
  `total_indices`, `materials`, `position_bounds`. `decode_skeleton` is the cheap
  bone-name-only path.
- `DecodeError` variants: `Kv3(&str)`, `Meshopt(&str)`, `Model(&str)`,
  `Truncated{offset,needed,had}`, `UnsupportedKv3(u32)`.
- `material::parse(&[u8]) -> Material` (M4). `Material { name, shader_name,
  texture_params, int_params, float_params, vector_params }`, `Material::pbr()`
  -> `PbrSlots`, `alpha_mode()`, `alpha_cutoff()`, `texture(slot)`.
- `model::to_glb(&Model) -> Result<Vec<u8>, DecodeError>` (M5a, untextured) and
  `model::to_glb_textured(&Model, &dyn model::FileResolver)` (M5b, textured).
  Both emit the model's animation clips (M7). `FileResolver::resolve(compiled_path)
  -> Option<Vec<u8>>` is the caller's VPK I/O hook. `math.rs` has
  `Mat4::from_scale` + `Quat::from_yaw_pitch_roll`; `MeshPart::bone_weight_count`
  is stored for default weights.
- `Model { skeleton, meshes, animations }` (M7 added `animations: Vec<Clip>`).
  `Clip { name, fps, frame_count, looping, tracks: Vec<BoneTrack> }`;
  `BoneTrack { bone, translations, rotations, scales }` (each `Option<Vec<_>>`,
  length `frame_count`). `vpkmerge-core::AnimOptions { no_anim, clips }` filters
  the clip list before `to_glb_textured` (CLI `--no-anim` / `--clip <name>`).
- M6 orchestration sketch: `vpkmerge-core` opens the skin VPK (+ base pak), reads
  the entry, calls `model::decode`, then `to_glb_textured` with a `FileResolver`
  that tries the skin VPK then base. Mirror the cross-VPK pattern in the gated
  `morphic/tests/model_local.rs::VpkResolver`. CLI: extend `ModelCmd` with
  `export --vpk --entry --base --out`. Then refresh the bundled binary.
- Texture decode (`morphic::decode` / `decode_at`) is done; M5 reuses it to turn
  the `PbrSlots` `.vtex` paths (append `_c`) into glTF images. Mirror
  `vpkmerge-core/src/portrait.rs` for the M5/M6 cross-VPK loader + orchestration.

## Decisions / notes

- RESOLVED: the "CTRL is the buffer registry" correction + verified
  `embedded_meshes` ordering are now folded into `vmdl-glb-exporter-handoff.md`
  (its M3 section), so the scope authority is no longer misleading.
- Validation split (kept as-is): committed CI tests validate buffer-free
  structure (skeleton, layouts, draw calls, scene bounds); byte-level mesh decode
  is validated by the gated `tests/model_local.rs` (buffers are multi-MB, not
  committed) and will be re-confirmed by the M5 GLB semantic diff. A committed
  end-to-end assembly test is feasible from `gun_lod3` (the one complete small
  mesh already in `fixtures/meshopt/`) if we later want CI to exercise the actual
  deinterleave; deferred as optional hardening.
