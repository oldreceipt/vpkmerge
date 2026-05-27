# Handoff: add animation (clip) export to `vpkmerge model export`

You are extending the Source 2 model exporter in **vpkmerge**. Work on branch
**`feat/vmdl-glb-export`**; create a worktree off it and build with
`cargo build --release -p vpkmerge-cli`.

## Goal

`vpkmerge model export` today emits a static mesh + skeleton + skin + materials
but **no animations**. Decode the Source 2 animation blocks (`ANIM`/`ASEQ`/`AGRP`)
from a hero's own `.vmdl_c` and emit them as **glTF animation clips**, so the
exported `.glb` plays that hero's own sequences (idle, etc.) on its own skeleton.

## Why per-hero own-animations, NOT a shared retarget clip

Verified: Deadlock heroes do **not** share one skeleton. In-game Vindicta
(`hornet`) = 62 bones; Paige (`bookworm`) = 164 bones; only ~40 bone names in
common. So the old plan ("ship one `hornet_idle.glb` and retarget it onto every
hero by bone name") is dead. Each hero's clips must come from that hero's own
model and drive its own skeleton: identical bind pose, no retargeting.

## Current code (all of this is a faithful port of ValveResourceFormat / VRF)

- `morphic/src/model/mod.rs` — `decode()` -> `Model { skeleton, meshes }`.
  `inspect()` already detects `ANIM`/`ASEQ`/`AGRP` (constants ~line 194). `Model`
  is what the glb writer consumes; add the decoded clips here (e.g. a new
  `animations: Vec<Clip>` field, or a sibling `decode_animations()`).
- `morphic/src/model/skeleton.rs` — `Skeleton`/`Bone`, ported from VRF
  `Skeleton.FromModelData` + `Bone`. Bones carry `name`, `parent`,
  `position`/`rotation` (parent-space), and local/global/inverse bind matrices.
  Animation tracks target these bones (by index, matched via name).
- `morphic/src/model/glb.rs` — `to_glb` / `to_glb_textured` / `build()`, using
  the `gltf-json` crate. Emits nodes/skin/accessors/bufferViews; the skin's joint
  node indices are built in `SkinRefs` (~line 121). **There is no `animations`
  array yet — that is where clips get emitted.** Read its header comment: bone
  local transforms are kept in **Source space**, and a `TRANSFORMSOURCETOGLTF`
  wrapper node sits ABOVE the skeleton root (inches->meters, Z-up->Y-up).
- `morphic/src/kv3/` — KV3 binary parser (the `AGRP`/`DATA` payloads are KV3;
  `ANIM` carries KV3 + compressed segment buffers).
- `morphic/src/resource.rs` — block-table access (`find_block`, `blocks()`).

## The decode work (the hard part): port VRF `ResourceTypes/ModelAnimation`

Reference implementation (same source we ported meshopt + KV3 from):
`https://github.com/ValveResourceFormat/ValveResourceFormat` ->
`ValveResourceFormat/Resource/ResourceTypes/ModelAnimation/`
(`Animation.cs`, `AnimationClip.cs`, `AnimationFrameBlock.cs`,
`AnimationSegmentDecoder.cs`, `Frame.cs`, and the `Decoders/` folder). Match its
math exactly.

1. **`AGRP`** (animation group, KV3): `m_decoderArray` (segment decoder names),
   `m_segmentArray`, `m_localHAnimArray` / anim file refs, and the decode key
   (bone list / remap). In newer Deadlock compiles the skeleton decode info can
   live here; reconcile against the `DATA`-block skeleton already parsed.
2. **`ANIM`**: per-clip frame data, segmented. Each segment = a decoder index, a
   channel (`Position` / `Angle` / `Scale` / `Data`), a bone wildcard list, and
   compressed per-frame values.
3. **`ASEQ`**: sequences -> named clips (fps, frame count, looping flags, which
   anim(s) they reference). These names become the glTF clip names (e.g.
   `primary_stand_idle`, `idle_loadout`).
4. **Segment decoders to port** (VRF concrete classes): `CCompressedStaticFullVector3`,
   `CCompressedFullVector3`, `CCompressedAnimVector3`, `CCompressedDeltaVector3`,
   `CCompressedStaticVector3`; quaternion: `CCompressedAnimQuaternion`,
   `CCompressedFullQuaternion`, `CCompressedStaticQuaternion` (the 6-byte packed
   quat: 2-bit largest-component index + three 15-bit components, sign rules per
   VRF); float: `CCompressedStaticFloat`, `CCompressedFullFloat`.
5. **Assemble per clip**: for each animated bone, a track of (time, T/R/S)
   keyframes at the clip fps. Bones not animated by a clip keep their bind pose.

## The emit work (`morphic/src/model/glb.rs`)

- Add an `animations` array to the glTF doc; one entry per clip (name = sequence
  name).
- Per animated bone+channel: a **sampler** (input accessor = times in seconds =
  `frame / fps`, SCALAR f32; output accessor = VEC3 for translation/scale, VEC4
  quaternion for rotation; `interpolation: LINEAR`) and a **channel** whose
  `target.node` is the skin joint node for that bone (reuse `SkinRefs`) and
  `target.path` is `translation` | `rotation` | `scale`.
- **Coordinate space (critical):** the bind-pose node TRS already emitted is in
  Source/local space (the source->gltf transform lives on the wrapper node ABOVE
  the skeleton). Emit animation TRS in the SAME raw Source/local space; do NOT
  pre-apply the source->gltf transform to keyframes. Confirm against the golden.

## Testing: use the existing VRF golden harness

- `vpkmerge/tools/morphic-oracle/` (C# + VRF) already has `model` (golden glb)
  and `kv3-dump` subcommands (commit `aed2240`, with hornet KV3 fixtures).
  Extend it to emit VRF's own animated glTF for the same `.vmdl_c`, then diff
  morphic's clip count / clip names / per-bone keyframe values against it.
- `cargo test -p morphic` for the existing static-export tests; add animation
  cases under `morphic/src/model/tests.rs`.

## Golden model to test against (and the trap to avoid)

In-game Vindicta: entry `models/heroes_staging/hornet_v3/hornet.vmdl_c` inside
`~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk` (62-bone
rig with `ANIM`/`ASEQ`; expected clip set is what the existing bundled
`grimoire/public/models/hornet_idle.glb` contains: `idle_loadout`,
`primary_stand_idle`, `ui_hero_select`, `ui_main_menu`, `ui_hero_pose`, etc.).

**Do NOT use `hornet_backup.vmdl_c`** (the mod author's HD backup: a malformed
3-skin / no-skeleton-root rig). That is the source of the current broken bundled
`public/models/hornet*.glb`.

## Verify visually

```bash
cargo build --release -p vpkmerge-cli
BASE=~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk
./target/release/vpkmerge model export --vpk "$BASE" --hero hornet --base "$BASE" --out /tmp/hornet_anim.glb

# viewer (Wayland): up/down cycle clips, Esc quits
cd ~/aaplsucks && WAYLAND_DISPLAY=wayland-1 XDG_RUNTIME_DIR=/run/user/1000 \
  ./target/release/glb_viewer /tmp/hornet_anim.glb
```

Expect `[glb_viewer] loaded ... (skin: true, N animation(s))` with N > 0 and the
idle playing without distortion.

## Acceptance

- `model export` emits all of a model's sequences as named glTF clips by default
  (add a `--clip <name>` filter and/or `--no-anim` flag, but default = all).
- Clip names + count match the VRF golden; idle plays correctly in `glb_viewer`.
- `cargo test -p morphic` green (static export unchanged).
- Follow-up (separate step): re-export the bundled `grimoire/public/models/*.glb`
  from correct in-game models so the desktop viewer's example assets are good,
  and drop the cross-hero retarget assumption in `HeroModelViewer.tsx`
  (per-hero clips now ship inside each hero's own GLB).

## Gotchas

1. Skeleton lives in `DATA` today but can move to `AGRP` in newer compiles —
   handle both, reconcile bone order/indices.
2. The Source 2 packed quaternion (6 bytes) needs the exact sign/scale + largest-
   component reconstruction from VRF; a near-miss looks like subtle joint jitter.
3. fps and frame count come from `ASEQ`/`ANIM`, never assume 30.
4. Keep everything in Source space to match the existing bind-pose emission, or
   mesh and animation will disagree.
