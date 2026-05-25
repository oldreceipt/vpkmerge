# `.vmdl_c -> .glb` exporter: progress + continuation brief

Live handoff state for the model exporter work. Resume point: **M3**. Scope
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

Remaining: **M3** mesh assembly + skeleton + skin, then **M4** materials, **M5**
GLB writer, **M6** CLI + core orchestration + refreshed bundled binary.

- M1 detail: ported from VRF `BinaryKV3.cs` KV3_V5 path; LZ4 via `lz4_flex`;
  `parse()` is crate-private. v1-v4 / ZSTD / binary-blob sections return errors
  (not needed: every hornet KV3 block is v5).
- M2 detail: `decode_vertex_buffer` / `decode_index_buffer`, scalar ports of
  zeux/VRF. Pure-Rust on purpose (no C toolchain). Oracle `mesh-buffers` added.

## CRITICAL structural facts (the handoff doc is wrong about buffer descriptors)

The handoff says vertex/index count, stride, and layout come from the MDAT KV3.
They do NOT. Reality:

- The **`CTRL` block is the buffer registry**. Its root has `embedded_meshes[]`
  (10 for hornet = LOD groups: `body`, `body_lod1..3`, `gun`, `gun_lod1..3`,
  `ghost_glow`, `ghost_glow_lod1`). Each entry: `m_Name`, `m_nDataBlock` (the
  MDAT block index), `m_vertexBuffers[]`, `m_indexBuffers[]`. Each buffer
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

## M3 to-dos

- Fetch VRF `DXGI_FORMAT` enum (seen `m_Format` ints: 2, 6, 16, 28, 30).
- Deinterleave the decoded vertex stream per `m_inputLayoutFields`.
- Port VRF `VBIB.cs` helpers: `GetNormalTangentArray` (compressed-normal formats
  R8G8B8A8_UNORM and R32_UINT), `GetBlendIndicesArray` + bone remap table,
  `GetBlendWeightsArray`.
- Map draw calls to glTF primitives (index range + material index).
- Read `m_skeleton`: bone names, parent hierarchy, bind pose, inverse-bind
  matrices. Map per-vertex BLENDINDICES to global bone indices.
- Validate: joint count and the SET of bone names match the golden skin (bone
  names are the load-bearing retarget key for Grimoire's shared clips).

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
  `mesh-buffers --vpk --entry --out-dir`. Justfile: `just model-golden <entry>
  <out>`, `just kv3-goldens`, `just mesh-buffers`.
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
- `Resource::parse`, `find_block([u8;4]) -> Option<&[u8]>`, `blocks()`. M3 needs
  block-by-global-index for `m_nBlockIndex`; add a `get_block_by_index(n)`
  accessor if missing.
- `DecodeError` variants: `Kv3(&str)`, `Meshopt(&str)`,
  `Truncated{offset,needed,had}`, `UnsupportedKv3(u32)`.
- Texture decode (`morphic::decode` / `decode_at`) is done and reused for M4
  materials. Mirror `vpkmerge-core/src/portrait.rs` for M6 orchestration.

## Open question for the user (from the M2 checkpoint)

Whether to fold the "CTRL is the buffer registry" correction into
`vmdl-glb-exporter-handoff.md`.
