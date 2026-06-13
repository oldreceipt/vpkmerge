# Handoff: pose WIP heroes from their loose NM clips (`.vnmclip_c`)

Recon + implementation for the "Issue 3" hero-card export gap (2026-05-29).
**Status: implemented and verified against local Deadlock.** Every blocker the
triage feared (anim-graph traversal, an `agrp` resolver, a compressed-stream
decoder) evaporated for the static menu pose. The recon below records why; the
"Implementation" section records what shipped.

## The problem (restated)

Six WIP heroes export with **0 animations** and `--require-pose` drops them to a
2D portrait: Apollo (`fencer`), Billy (`punkgoat`), Celeste (`unicorn`),
Mina (`vampirebat`), Paige (`bookworm`), Rem (`familiar`). morphic only reads
clips **embedded** in the `.vmdl_c` (`ANIM`/`AGRP`/`ASEQ`). These heroes embed
none; their clips ship as **loose files**.

The triage assumed the loose clips were legacy `.vanim_c`/`.vagrp_c` behind an
animation graph. **They are not.** They are the newer **NM** (motion-matching)
format: `models/heroes_wip/<h>/clips/*.vnmclip_c` plus one
`models/heroes_wip/<h>/<h>.vnmskel_c`. No `.vagrp_c`/`.vanim_c` exist.

## What the recon established

### 1. The NM files are plain KV3 (already decodable)

Both `.vnmclip_c` and `.vnmskel_c` are standard Source 2 resources whose `DATA`
block is binary KV3. `morphic::decode_kv3_resource` reads them today, no new
container/LZ4 work. (`.vnmclip_c` carries `RERL`/`RED2`/`DATA`; `.vnmskel_c`
carries `RED2`/`DATA`.)

### 2. The menu-pose clip is a single, fully-static frame

`fencer/clips/ui_hero_select.vnmclip_c`:
- `m_nNumFrames = 1`, `m_flDuration = 0`, `m_bIsAdditive = false`
- `m_compressedPoseData = 0 bytes`
- `m_trackCompressionSettings = [99 tracks]`, **all 99 fully static**
  (`m_bIsRotationStatic && m_bIsTranslationStatic && m_bIsScaleStatic`).

So the whole pose reconstructs from `m_trackCompressionSettings` alone. **There
is no compressed stream to dequantize and no graph to follow.** Per static
track, the constant transform is:
- rotation = `m_constantRotation` (a `[qx,qy,qz,qw]`)
- translation = `(m_translationRangeX, ...Y, ...Z).m_flRangeStart`
- scale = `m_scaleRange.m_flRangeStart`

(`m_flRangeLength` is the quantization width; unused for a static track. For a
future *animated* track you would read `m_compressedPoseData` at
`m_compressedPoseOffsets[frame]` and dequantize within `[start, start+length]` —
**not needed for any current menu pose**.)

Verified against the skeleton bind pose (`examples/vnmverify.rs`): static
translation/scale equal `m_flRangeStart`, and **73/99 bones rotate away from
bind** (head turn, curled fingers, weapon placement), so `ui_hero_select` is a
genuine authored menu pose, not a no-op.

### 3. The NM skeleton maps onto the mesh skeleton by name

`.vnmskel_c` `DATA`:
- `m_boneIDs = [N names]` (ordered; the clip's track `i` is bone `i`)
- `m_parentIndices = [N]`
- `m_parentSpaceReferencePose = [N][8]`, layout **`[px,py,pz, scale, qx,qy,qz,qw]`**
- `m_modelSpaceReferencePose = [N][8]` (same layout)

The NM bones are a **subset of the model's embedded mesh skeleton, by exact
name** (fencer 99 ⊆ 365; bookworm 104 ⊆ 164). The model's extra bones are
`$cloth_m0p*` sim bones and `*_TWIST`/helper/`attach*` bones the clip never
drives. So the existing **by-name** baking applies directly: bones the clip
names get posed; cloth/twist/helper bones gracefully keep their bind transform
(correct for a static card).

### 4. It generalizes across all six heroes

| hero (codename) | menu clip | bones | shape |
|---|---|---|---|
| Apollo `fencer` | `ui_hero_select` | 99 | 1 frame, all static |
| Billy `punkgoat` | `ui_hero_select` | 103 | 1 frame, all static |
| Celeste `unicorn` | `ui_hero_select` | 122 | 1 frame, all static |
| Paige `bookworm` | `ui_hero_select` | 104 | 1 frame, all static |
| Rem `familiar` | `ui_hero_select` | 126 | 1 frame, all static |
| Mina `vampirebat` | `vampirebat_ui_pose` / `vampirebat_hero_pose` | (NM) | 1 frame, all static |

**Naming caveat:** five heroes use the bare `ui_hero_select.vnmclip_c` (already
in `DEFAULT_POSE_CLIPS`). Mina prefixes the hero codename:
`vampirebat_ui_pose`, `vampirebat_hero_pose`, `vampirebat_ui_hero_info`,
`vampirebat_bindpose`. The loose-clip resolver must try both the bare candidate
names and `<codename>_<candidate>` (and likely a bare `*_ui_pose`/`*_hero_pose`
suffix match).

## Implementation (shipped)

1. **morphic: NM decode** (`morphic/src/model/nm.rs`).
   - `decode_nm_skeleton(&[u8]) -> NmSkeleton` (ordered bone names).
   - `decode_nm_pose(&[u8]) -> NmPose`: per-bone constant local transform from
     `m_trackCompressionSettings` (rotation = `m_constantRotation`,
     translation/scale = the channel `m_flRangeStart`s). A non-static track
     decodes to `None` for that bone (the baker keeps its bind), so an
     unexpected animated track degrades gracefully instead of rendering wrong.
   - `bake_nm_pose(model, skel, pose) -> Model`: zips the pose's tracks with the
     skeleton's bone names and bakes via `pose::bake_pose_named`, the by-name
     generalization of `bake_pose_from` (FK + skinning on the *model* skeleton,
     so the static output is a plain posed mesh, no skeleton/skin/clips).
2. **vpkmerge-core: loose-clip resolution** (`bake_loose_nm_pose` in `model.rs`),
   wired into `export_resolved` as a third pose source after embedded clips and
   the base-pak donor. For each candidate it tries `<dir>/clips/<cand>.vnmclip_c`
   and the `<stem>_<cand>` variant (Mina prefixes the codename), reads the
   `.vnmskel_c` the clip's `m_skeleton` references, and bakes. It wins over a
   clipless donor but never over a real embedded/donor clip, so `--require-pose`
   now passes for these heroes instead of dropping to a 2D portrait.
3. **Candidate names.** `DEFAULT_POSE_CLIPS` already lists `ui_hero_select` and
   `hero_pose`; the `<stem>_<cand>` lookup covers Mina (`vampirebat_hero_pose`).

### Verified against local Deadlock
`model export --pose --require-pose` now succeeds (was: bailed to portrait) for
Apollo/Paige/Mina. Per hero, the NM bones are a by-name subset of the mesh
skeleton with **every NM bone's model parent also an NM bone** (99/99, 104/104,
162/162), so FK is consistent; the baked pose displaces 97-100% of vertices from
bind. Regression coverage: `morphic/tests/nm_local.rs` (gated on
`MORPHIC_MODEL_VPK`, run in the `just` daily loop) asserts the static-ness,
parent-consistency, and displacement invariants; CI-safe unit tests in
`nm.rs`/`tests.rs` cover the KV3 decode and the by-name bake.

### Known limitations
- Cloth/twist/helper bones not in the NM clip stay at bind pose in the still
  (same limitation the existing donor-pose path already has).
- The hero-card still-pose path (`decode_nm_pose`/`bake_nm_pose`) reads only the
  static per-track constants, which is the whole pose for every menu/idle clip
  (they are fully static). Animated clips are now handled by a separate codec:

### Update (2026-06-13): the quantized-pose codec landed

`m_compressedPoseData` is now decoded and re-encoded. `morphic::model`:
- `decode_nm_clip(bytes) -> NmClip`: per-bone `NmTrack`s, each with the static
  `TrackSettings` plus a per-frame `Vec` for every *animated* rotation /
  translation / scale channel (`None` for a static channel, whose constant is in
  the settings). Static clips decode with every channel vector `None`, identical
  in spirit to `decode_nm_pose`.
- `encode_compressed_pose(&NmClip) -> (Vec<u8>, Vec<u32>)`: re-quantizes to the
  `(m_compressedPoseData, m_compressedPoseOffsets)` pair, the exact inverse.
- `decode_pose_stream(settings, data, offsets, frames)`: re-decode helper for
  round-trip proofs.

Faithful port of VRF `ModelAnimation2/AnimationClip` (`ReadFrame`,
`DecodeQuaternion` = 3-word "smallest three", `DecodeTranslation`/`DecodeFloat`
= 16-bit unorm across `[start, start+length]`). Per frame, at the frame's `u16`
offset, each track contributes 3 words for an animated rotation, 3 for a
translation, 1 for a scale, in `m_trackCompressionSettings` order.

Verified across the whole pak (`tests/nm_clip_local.rs`, gated on
`MORPHIC_MODEL_VPK`): all 9008 animated clips re-encode with translation and
scale **byte-exact** and rotation within **0.0012 rad** worst case; **90.7%**
re-encode byte-for-byte. The ~9% that do not are the smallest-three quaternion's
inherent largest-component tie (two near-equal components let an equivalent
3-word encoding be chosen) and are pose-identical, not a codec error. Committed
fixtures + CI round-trip: `tests/nm_clip.rs` over `fixtures/nm/*.vnmclip_c`.

### First authored custom animation (2026-06-13, pending in-game verify)

`vpkmerge-core/examples/yamato_custom_pose.rs` authors the first hand-made
Deadlock pose: it edits Yamato's static `ui_hero_select` clip (raise
`arm_upper_L` 75 degrees, tilt `head` 25, lean `spine_2` 20) by **byte-faithfully
patching the `m_constantRotation` quaternions in place**
(`patch_kv3_resource_doubles`/`_floats`, the same structural patch the vpost /
material recolors use, so the v5 envelope is preserved), then packs the edited
clip at her `reload_idle` + `reload_idle_quick` paths (the proven press-R taunt
slots from the experiment rounds). Offline it bakes the edited pose onto Yamato's
mesh (`bake_nm_pose`): 58.5% of vertices move from the unedited pose (max ~29u),
and a re-decode of the patched clip confirms the targeted rotations, so the edit
is well-formed. Staged addon: `.scratch/yamato_taunt/yamato_reload_taunt_dir.vpk`
(+ a `yamato_custom_pose.glb` to eyeball). Install as a free `pakNN_dir.vpk` in
`game/citadel/addons/` and press R. (This edits a *static* clip, so it exercises
the KV3 patch path, not the new compressed-pose codec; an animated-clip edit
would splice an `encode_compressed_pose`d stream of equal length in place.)

- Several heroes ship alternate UI face meshes (`head_ui_smug`/`_cocky`/...) and
  Apollo a long sword; the export includes all parts. Trimming alternate faces to
  one is a separate hero-card polish item (cf. the Viscous alt-form drop), not a
  posing concern.

### Recon method (tools were scratch, not committed)
The findings above were established with one-off `cargo run --example` inspectors
(block-table dump + DATA-as-KV3, static-vs-animated track counts, static-constant
vs bind validation, model-vs-`vnmskel` bone-name overlap, and posed-vs-bind
vertex displacement). They are trivially reconstructed from
`morphic::decode_kv3_resource` + `morphic::model::{decode,decode_nm_pose,
decode_nm_skeleton,bake_nm_pose,bake_pose}`; the durable assertions live in
`morphic/tests/nm_local.rs`.
