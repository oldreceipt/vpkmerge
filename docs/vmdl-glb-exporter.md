# vmdl_c -> glTF exporter

Status: in progress. Goal: turn a Deadlock hero `.vmdl_c` (Source 2 compiled
model) into a standard `.glb` so it can be rendered outside the game. The
immediate consumer is a richer Grimoire Locker background (a high-res, full-body
render of the *installed skin*, not the base hero), rendered by the `esox`
engine (`~/aaplsucks`) which already ingests glTF + does PBR + skinning.

## Why this is feasible (what the probe proved)

Probed against `bunnydicta_bunnysuit_vindicta_dir.vpk` (a Vindicta / `hornet`
skin):

1. **The mesh is embedded in the mod.** Both `vmdl_c` entries carry `MVTX`
   (vertex) / `MIDX` (index) / `MDAT` blocks directly. No dependency on
   Deadlock's multi-GB base VPKs. The mod is self-contained for geometry. This
   was the single biggest risk and it is dead.

2. **Both compression formats are standard, not proprietary:**
   - `MVTX` first byte `0xa1` -> meshoptimizer vertex codec v1
   - `MIDX` first byte `0xe1` -> meshoptimizer index codec v1
   - `MDAT` / `DATA` / `vmat_c` start with `05 33 56 4b` ("3VK") -> binary KV3,
     magic `0x4B563305` (KV3 v5, the newest variant)

   meshoptimizer is zeux's open codec (decode is two functions; a maintained
   Rust `meshopt` crate exists). KV3 is Valve's KeyValues3 binary format, the
   same one ValveResourceFormat (VRF) parses. Neither is a black box.

3. **There are two models in the mod:** `hornet.vmdl_c` (1.75 MB, 8 mesh parts)
   and `hornet_backup.vmdl_c` (7.68 MB, ~18 parts, extra `DSTF` cloth +
   heavy `ANIM`). The first is almost certainly the in-game model; the backup
   is the author's HD copy. **We target `hornet.vmdl_c` first.**

## What already exists (do not rebuild)

| Piece | Where | State |
|---|---|---|
| VPK open / list / read | `valve_pak` crate (used in `portrait.rs`) | done |
| S2 resource container (header + block table) | `morphic::resource` | done |
| `.vtex_c` decode (BCn / RGBA / embedded PNG) -> RGBA | `morphic::texture` | done |
| KV3 `Value` tree types + accessors | `morphic::kv3::types` | done |
| KV3 binary `parse()` | `morphic::kv3::parse` | **STUB** ("not yet implemented") |
| glTF *read* + PBR + skeletal anim + GPU skinning | `esox_gfx` (`~/aaplsucks`) | done |
| Offscreen render target | `esox_gfx::offscreen` | done (no PNG readback yet) |

## Architecture / crate layering

```
morphic            Source 2 decode + glTF emit (pure, no I/O beyond bytes)
  resource         block table                [done]
  texture          vtex_c -> RGBA             [done]
  kv3              KV3 binary -> Value tree    [M1]
  model            vmdl_c -> mesh/mats/skel    [M2-M3]
    -> glb         assemble + write .glb       [M3]
vpkmerge-core      orchestration: VPK in, .glb out (mirrors extract_portraits)
vpkmerge-cli       `vpkmerge model ...` subcommand
esox (~/aaplsucks) load .glb, render turntable, headless PNG frames  [M4]
Grimoire           show frames as Locker background                  [M5]
```

The exporter's output is a **standard `.glb`**: validate it in Blender or any
glTF viewer before any esox involvement. That keeps the risky decode work in a
tight, independently verifiable loop.

## Source 2 block reference (hero vmdl_c)

| Block | Meaning | Needed for export? |
|---|---|---|
| `MVTX` | meshoptimizer-compressed vertex buffer (one per mesh part) | yes |
| `MIDX` | meshoptimizer-compressed index buffer | yes |
| `MDAT` | KV3: per-mesh render data (vertex layout, draw calls, material per call) | yes |
| `DATA` | KV3: model-level (mesh/material groups = skins, skeleton ref, attachments, LODs) | yes |
| `CTRL` | KV3: render-mesh control / scene metadata | maybe |
| `AGRP` | KV3: animation group (skeleton lives here in newer format) | skeleton |
| `ANIM` / `ASEQ` | animation clips / sequences | animation only (post-v1) |
| `PHYS` | collision | no |
| `DSTF` | cloth / soft-body deformation | no |
| `RERL` | external resource refs (lists the `vmat_c` paths) | material resolve |
| `RED2` | edit/introspection info | no |

## KV3 v5 binary (port target for M1)

Pinned against `hornet.vmdl_c` (both its `MDAT` and `DATA` blocks):

- `magic = 0x4B563305` -> **KV3 version 5** ("\x053VK"), the newest variant.
- `format GUID = 7c161274 e9069846 aff2e63e b59037e7`.
- **`compression method = 1` -> LZ4**, `frame size = 16384`. ZSTD (method 2) is
  *not* used by this file, so M1 only needs LZ4 to handle Deadlock hero models;
  a complete reader should still branch on method (0=none, 1=LZ4, 2=ZSTD).
- First counts read cleanly (`bin_bytes`, `ints`, `eightbyte`); the layout
  *after* that diverges from older versions, so the exact v5 header tail +
  buffer reconstruction is ported from VRF `BinaryKV3.cs` (KV3_V5 path), not
  guessed.

```
u32   magic = 0x4B563305
[16]  format GUID
u32   compression method = 1 (LZ4)
u16   compression dictionary id = 0
u16   compression frame size = 16384
u32   count of binary bytes
u32   count of 4-byte ints
u32   count of 8-byte values
... (v5 tail: string/type buffer size, uncompressed/compressed sizes,
     block count + block sizes); LZ4-decompress the region in frame_size
     chunks, then reconstruct the Value tree from byte/int/double/string
     buffers driven by the type-tag stream.
```

Only new dep for M1: `lz4_flex` (pure-Rust LZ4 block decode).

## meshoptimizer decode (M2)

`MVTX` = `decodeVertexBuffer`, `MIDX` = `decodeIndexBuffer` (zeux codec). The
vertex *count*, vertex *stride*, index *count*, index *width* (2 or 4 bytes),
and the **attribute layout** (which bytes in the stride are POSITION / NORMAL /
TANGENT / TEXCOORD / BLENDINDICES / BLENDWEIGHT, and their component formats)
all come from the `MDAT` KV3. Decode the buffer, then deinterleave per layout.
Use the `meshopt` crate if its decode matches the v1 codec byte-for-byte;
otherwise port the two decode functions.

## Materials (M3)

`vmat_c` are KV3. Map the Deadlock shader's texture params to glTF PBR channels
(albedo -> baseColor, normal -> normalTexture, AO/roughness/metalness ->
occlusion/metallicRoughness). Textures referenced by the `vmat_c` are `vtex_c`
already in the mod and already decode via `morphic::texture`. "Looks right" is
subjective here; expect iteration on shader-slot mapping.

## Skeleton (M3)

Bind pose only for v1 (bone names, parent hierarchy, inverse bind matrices from
`DATA`/`AGRP`). Render the model in bind pose; do not animate. `ANIM`/`ASEQ`
exist so a posed/animated turntable is a later option, not a v1 requirement.

## Milestones

- **M0 - inspect (this slice).** `vpkmerge model inspect <vpk>`: list `.vmdl_c`
  entries and their blocks (mesh-part count, skeleton/anim/physics presence,
  vertex-buffer bytes) via `morphic::model::inspect`. Establishes the module
  wiring morphic -> core -> cli. Verifiable immediately.
- **M1 - KV3.** Implement `morphic::kv3::parse` for v5. Validate by dumping
  `hornet.vmdl_c`'s MDAT + DATA trees. Critical-path dependency for everything.
- **M2 - mesh -> OBJ.** meshopt-decode one `MVTX`+`MIDX`, deinterleave per MDAT
  layout, write `.obj`. Open in a mesh viewer; if the bunny silhouette appears,
  the meshopt + layout path is proven.
- **M3 - glb.** All mesh parts + materials (vmat -> PBR, textures via
  `morphic::texture`) + skeleton (bind pose) -> `.glb`. Validate in Blender.
- **M4 - render.** esox headless turntable: load `.glb`, render N frames to PNG
  (add buffer readback to `esox_gfx::offscreen`).
- **M5 - Grimoire.** Show frames/sprite-sheet as the Locker background, keyed by
  the installed mod's hero. Native renderer packaged for Windows + Linux.

## Risks / open questions

- **KV3 v5** is the meatiest single piece; VRF's BinaryKV3 is the reference.
- **Shader-slot -> PBR mapping** is fiddly and subjective per Deadlock shader.
- **Cross-platform packaging** of a native wgpu renderer alongside Electron
  (esox is currently Linux-only by choice; wgpu itself targets Windows).
- Pick `hornet.vmdl_c` (in-game) over `hornet_backup.vmdl_c` (HD) for v1.
- Bind-pose static render for v1; posing/animation deferred.
