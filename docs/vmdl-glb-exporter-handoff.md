# Handoff: `.vmdl_c` -> `.glb` exporter in vpkmerge

You are implementing a headless Source 2 model exporter inside `vpkmerge` so
Grimoire can render an installed Deadlock skin as a posed backdrop. This doc is
self-contained: read it, then `docs/vmdl-glb-exporter.md` (the deeper format
notes pinned against a real model). Where the two disagree, this doc wins on
scope; that one wins on byte-level format detail.

## Mission

Add `vpkmerge model export --vpk <skin.vpk> --entry <path.vmdl_c> --base <pak01_dir.vpk> --out <model.glb>`
that emits a valid binary glTF (`.glb`) containing the model's **mesh + skeleton
+ skinning + materials/textures**. It runs as the already-bundled native
`vpkmerge` binary (no new runtime on user devices). Grimoire's renderer
(three.js) loads the `.glb`, retargets shared animation clips onto its skeleton,
poses it, and bakes a still. You do NOT touch the Grimoire/Electron side.

## Scope (read this before writing code)

IN:
- KV3 binary parsing (the gate; currently stubbed).
- Mesh decode: `MVTX`/`MIDX` (meshoptimizer) + `MDAT` (layout/draw calls).
- Skeleton + per-vertex skin weights (glTF `skin` with inverse-bind matrices).
- Materials + textures (`RERL` -> `.vmat_c` -> `.vtex_c`, decode + embed).
- Cross-VPK resolution: skin VPK first, fall back to the base `pak01_dir.vpk`.
- `.glb` writer + CLI subcommand + core orchestration.

OUT (do not build):
- **Animation clip decode** (`ANIM`/`ASEQ`). Grimoire's clips are a separate,
  bundled GLB retargeted by bone name, so per-skin GLBs need the **skeleton but
  no animations**. You must still emit named bones / a correct skin; just no
  animation samplers.
- **esox** (the renderer in `~/aaplsucks`). Off the critical path. You only emit
  a `.glb`; three.js renders it. Ignore milestones M4/M5 in the old design doc.
- Physics (`PHYS`), cloth (`DSTF`), LODs beyond LOD0.

## Where things are

Repo root: `/home/esoc/grimoire-workspace/vpkmerge`

| Piece | Path | State |
|---|---|---|
| Decode crate | `morphic/src/` | textures done; model decode missing |
| Resource container + block table | `morphic/src/resource/` | DONE. `Resource::parse`, `find_block(kind) -> Option<&[u8]>`, `blocks()`, `raw()`, `data_block()` |
| `.vtex_c` decode (BCn/RGBA/inline) | `morphic/src/texture/` | DONE. Reuse for material textures |
| KV3 `Value` tree + accessors | `morphic/src/kv3/types.rs` | type model exists (`Value`, `get`, `as_int`, ...) |
| **KV3 binary parser** | `morphic/src/kv3/mod.rs` | **STUB** (`parse` returns `Err("kv3::parse not yet implemented")`) |
| Model structural read | `morphic/src/model/mod.rs` | `inspect()` only (block histogram, counts) |
| Core orchestration | `vpkmerge-core/src/model.rs` | `inspect_models()` only. Mirror `portrait.rs` for export |
| CLI | `vpkmerge-cli/src/main.rs` | `Command::Model(ModelCmd { input })` -> `run_model` is inspect-only. Add export flags/subcommand |
| Bundled binary Grimoire runs | `../grimoire/resources/vpkmerge/vpkmerge-linux-x86_64` | rebuilt by `cargo build --release` + copy |

Getting block bytes for decode is already possible: `Resource::find_block(*b"MDAT")`,
`find_block(*b"DATA")`, `find_block(*b"AGRP")`, `find_block(*b"RERL")`, etc.

## Environment / test data (all present on this machine)

- Rust: workspace builds with `cargo build` from repo root.
- Base game pak (material/skeleton resolution + a fixture source):
  `~/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`
  (multi-archive; `valve_pak` opens the `_dir` and handles the `_NNN` parts).
- Dev addons (installed skins, may be in `.disabled/`):
  `~/.config/grimoire/dev-deadlock/game/citadel/addons/`
- **Known-good reference outputs** (made by Source2Viewer-CLI from the Vindicta
  bunnysuit skin): `../grimoire/public/models/hornet.glb` (mesh) and
  `hornet_idle.glb` (the shared clips). Diff your output against `hornet.glb`.

## Validation: the oracle (do this first, it de-risks everything)

`tools/morphic-oracle/` is a .NET 10 tool wrapping ValveResourceFormat (VRF),
the same engine Source2Viewer-CLI uses. It currently has `generate`/`extract`/
`survey` for textures only. **Add a `model` subcommand** that produces the golden
`.glb` you diff against:

- .NET SDK: `~/.dotnet/dotnet` (10.0.300; `global.json` pins it). Run from
  `tools/morphic-oracle/`: `~/.dotnet/dotnet run -- model --vpk ... --entry ... --base ... --out golden.glb`.
- VRF 19.1.6199 is restored (lockfile committed). The export API is present and
  documented in `~/.nuget/packages/valveresourceformat/19.1.6199/lib/net10.0/ValveResourceFormat.xml`:
  - `ValveResourceFormat.IO.GltfModelExporter` (ctor takes an `IFileLoader`;
    `set_ExportMaterials`, `set_ProgressReporter`; `ExportToFile(name, targetPath, resource)`).
  - `ValveResourceFormat.IO.GameFileLoader` (use it as the `IFileLoader`, give it
    the base `pak01` package so `.vmat_c`/`.vtex_c`/skeleton references resolve).
  - Read the `.vmdl_c` bytes with `Resource.Read(stream)`, pass to `ExportToFile`.
  Mirror the existing `Extract`/`Generate` subcommands in `Program.cs` for arg
  parsing and `ValvePak` usage.

GLBs differ byte-for-byte between writers, so **diff semantically**, not by bytes.
Write a Rust integration test (read both with the `gltf` crate) asserting, within
tolerance: vertex count, index/triangle count, joint count, the set of bone
names, per-mesh material count, and the model bounding box. Add a quick
`--validate` path or just lean on a glTF validator + loading the file in a glTF
viewer for a manual visual check (you have no GUI here; a maintainer does the
final eyeball).

## First fixture

Use a **base hero model from `pak01_dir.vpk`** (always available, self-contained
against the base pak). Find one: `cargo run -p vpkmerge -- model <pak01_dir.vpk>`
lists every `.vmdl_c`; pick a humanoid hero (e.g. the vanilla `hornet`/Vindicta
model under `models/heroes_staging/...`). Generate its golden GLB with the oracle,
then build the Rust path to match. The bunnysuit mod VPK + `hornet.glb` is a good
second fixture for the cross-VPK (mod overrides base) path.

## Work plan (dependency order)

Each milestone ends green against the oracle golden before the next starts.

### M1 - KV3 binary parser  (the gate; nothing works without it)
- Implement `morphic/src/kv3/mod.rs::parse` -> `Value`.
- Dispatch on the 4-byte magic. Deadlock hero models pin **KV3 v5**
  (`magic = 0x4B563305`), compression method 1 = **LZ4**, frame size 16384. Branch
  on method (0=none, 1=LZ4, 2=ZSTD) but only LZ4 is required for v1; also handle
  whatever older version (`v4`) any block in your fixtures uses.
- Port the v5 header tail + buffer reconstruction from VRF `BinaryKV3.cs`
  (KV3_V5 path) rather than guessing: LZ4-decompress in frame-size chunks, then
  rebuild the tree from the byte/int/double/string buffers driven by the
  type-tag stream.
- New dep: `lz4_flex` (pure-Rust LZ4 block). Add `zstd` only if a fixture needs it.
- Validate: parse `MDAT` and `DATA` of the fixture, serialize the `Value` to JSON,
  diff against a VRF KV3 dump (add an oracle `kv3-dump` subcommand, or print via
  VRF) for the fields you consume.

### M2 - meshoptimizer decode  (`MVTX` / `MIDX`)
- `MVTX` first byte `0xa1` = vertex codec v1; `MIDX` `0xe1` = index codec v1.
- Try the `meshopt` crate's `decode_vertex_buffer` / `decode_index_buffer` first;
  if its bytes don't match zeux's v1 codec exactly, port the two decode fns.
- Vertex count/stride, index count, index width (2 or 4) come from the `MDAT` KV3.
- Validate: decoded vertex/index counts equal the oracle GLB's accessor counts.

### M3 - mesh assembly + skeleton + skin
- From `MDAT`: vertex attribute layout (which stride bytes are POSITION / NORMAL /
  TANGENT / TEXCOORD / BLENDINDICES / BLENDWEIGHT, and component formats),
  deinterleave per layout; draw calls -> (index range, material index); material
  groups = skins (pick group 0 / default).
- From `DATA` (and `AGRP` if the skeleton lives there in this format): bone
  hierarchy, bind pose transforms, **bone names**, and compute inverse-bind
  matrices. Map per-vertex `BLENDINDICES` to global bone indices.
- Build an in-memory model: meshes/primitives with positions/normals/uv/joints/
  weights + a skeleton.
- Validate: joint count and the **set of bone names** match the oracle GLB's skin
  (bone names are the retarget key Grimoire relies on; they must match VRF's).

### M4 - materials + textures
- `RERL` lists external resource refs (the `.vmat_c` paths). Parse each `.vmat_c`
  (KV3) for its texture params -> `.vtex_c` paths.
- Resolve files via a **cross-VPK loader**: look in the input (skin) VPK first,
  then the `--base` `pak01_dir.vpk`. (Skin meshes are self-contained for geometry
  but usually reference base materials/skeleton.)
- Decode each `.vtex_c` with the existing `morphic` texture decoder; embed as glTF
  images. Map the model's PBR-ish params to glTF `pbrMetallicRoughness` +
  normal/occlusion as available. Base color + normal is enough for v1.
- Validate: material count + image dims match the oracle GLB.

### M5 - GLB writer
- Assemble glTF: nodes, meshes/primitives, accessors/bufferViews, a single
  packed buffer, a `skin` (joints + inverse-bind-matrix accessor), materials,
  embedded images. Write the GLB container (12-byte header + JSON chunk + BIN
  chunk).
- The `gltf` crate is read-only; use `gltf-json` to build the document and frame
  the GLB by hand, or another maintained writer if cleaner. Keep it a workspace
  dep, not vendored.
- Validate: passes a glTF validator; semantic diff vs the oracle golden is within
  tolerance; a maintainer confirms it loads + looks right in a glTF viewer and in
  Grimoire's three.js (it must skin + accept the bundled `hornet_idle.glb` clips
  retargeted by bone name).

### M6 - CLI + orchestration
- `vpkmerge-core/src/model.rs`: add `export_model(vpk, entry, base, out)` mirroring
  `portrait.rs` (open VPK, read entry, resolve via the loader, write `.glb`).
- `vpkmerge-cli/src/main.rs`: extend `ModelCmd` (keep inspect as the no-args form;
  add `export` with `--vpk/--entry/--base/--out`). Update `run_model`.
- Rebuild + refresh the bundled binary Grimoire ships:
  `cargo build --release` then copy `target/release/vpkmerge` to
  `../grimoire/resources/vpkmerge/vpkmerge-linux-x86_64` (and the Windows target
  when cross-building).

Grimoire-side wiring (an IPC that runs `vpkmerge model export` on the installed
skin VPK and drops the `.glb` into the user's library) is a **separate task in the
grimoire repo** and already has a home (`heroModels.ts` imports models today);
don't do it here. Just make the binary export correctly.

## Build & run

```bash
# from vpkmerge repo root
cargo build                       # debug
cargo test -p morphic             # unit + the new golden-diff integration tests
cargo run -p vpkmerge -- model <pak01_dir.vpk>            # inspect (list .vmdl_c)
cargo run -p vpkmerge -- model export --vpk <vpk> --entry <path.vmdl_c> --base <pak01_dir.vpk> --out /tmp/out.glb

# oracle golden (from tools/morphic-oracle)
~/.dotnet/dotnet run -- model --vpk <vpk> --entry <path.vmdl_c> --base <pak01_dir.vpk> --out /tmp/golden.glb
```

## Risks / gotchas

- **KV3 version drift.** Deadlock patches can change KV3 version or the
  `DATA`/`MDAT` schema. Branch on version; keep the VRF oracle handy to re-bless
  goldens after a game update. This is the standing maintenance cost.
- **Block-name variants.** Some models use `VBIB`/`MBUF`-style buffers instead of
  `MVTX`/`MIDX`; check your fixtures with `vpkmerge model <vpk>` and handle the
  variants Deadlock actually ships.
- **Bone names are load-bearing.** Grimoire retargets the shared clips by bone
  name, so your skeleton's names must match what VRF emits (and what
  `hornet_idle.glb` expects). Diff bone-name sets explicitly.
- **Non-humanoid heroes** (Mo & Krill, Viscous) have different skeletons; the
  shared clips won't retarget cleanly. Out of your scope (Grimoire handles pose
  selection), but don't assume one skeleton.
- **`InvariantGlobalization`/culture**, large buffers, and `u32` offset overflows:
  mirror the defensive parsing already in `resource/header.rs`.

## Definition of done

1. `vpkmerge model export` produces a `.glb` for a base hero model from `pak01`
   and for the bunnysuit mod VPK.
2. Both pass a glTF validator and load in a glTF viewer with correct mesh,
   skeleton (named bones), and base-color + normal materials.
3. The Vindicta output skins correctly and accepts the bundled `hornet_idle.glb`
   clips retargeted by bone name (maintainer confirms in Grimoire's 3D viewer).
4. Rust golden-diff integration tests (vs oracle GLBs) pass for the fixtures.
5. The refreshed bundled binary is committed for Grimoire to consume.
