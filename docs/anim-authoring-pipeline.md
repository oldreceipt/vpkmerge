# Custom animation authoring pipeline (end-state design)

Design note for the eventual "hand-author a Deadlock animation in Blender and play
it in-game" workflow. Captures where we are, the end-state UX, the engineering
still missing, and the constraints. Context: the quantized-pose codec for
`.vnmclip_c` is done and **in-game confirmed** (see
[handoff-nm-loose-clip-pose.md](handoff-nm-loose-clip-pose.md) and the NM section
of [../CLAUDE.md](../CLAUDE.md)); everything so far edits *existing* compiled clips
programmatically. This doc is the plan for true hand-authoring.

## Two layers

1. **Authoring (artist tools).** Blender or Maya: pose the rig, keyframe, IK, see
   the mesh deform. Produces a *source* animation.
2. **Compiled engine files.** Source 2's compiler (ModelDoc / resourcecompiler)
   turns that source into the compiled `.vnmclip_c` / `.vnmgraph_c`. This is the
   layer morphic reads and writes.

All work to date lives in layer 2 (decode the compiled quantized stream, transform
it, re-encode). Great for *derived* edits (amplify, spin, overlay, retime); not how
you key a dance from scratch, and limited to channels the clip already animates.

## The official pipeline (informational)

```
Blender/Maya -> FBX or DMX -> Source 2 ModelDoc -> resourcecompiler -> .vnmclip_c + graph
```

Deadlock has **no official modding/animation SDK released** (as of this work), so
this path isn't available to mods. And even with it, the animation graph resolves
clips by **compile-baked references** — late-added files at new paths are ignored
(proven; see the handoff doc's round 4). So a new clip can only enter the game by
**replacing an existing referenced clip at its path** — the same override mechanism
we already use. The SDK would only change *how the bytes are authored*, not *how
they get into the game*.

## End-state workflow (SDK-free, the target)

```
Blender (key against the exported skeleton)
  -> glTF
  -> morphic importer (sample per-frame per-bone, map by bone name)
  -> NmClip -> encode_compressed_pose
  -> write the compressed-pose section into the .vnmclip_c
  -> pack at the slot's path -> addon VPK
```

Artist's view, three commands:

1. **Export the rig to author against** (already built):
   ```
   vpkmerge anim export --hero yamato --slot reload_idle --out yamato_reload.glb
   ```
   A skinned, animated GLB: real mesh, real skeleton, the existing clip as
   reference. (`morphic::model::{decode, nm_clip_to_clip, to_glb}` +
   `examples/nm_clip_preview_glb.rs` do this today.)

2. **Animate in Blender.** Keyframe the armature. One rule: do not rename or
   reorder bones (mapping is by bone name). Export glTF with the animation.

3. **Import + compile + pack** (the missing command):
   ```
   vpkmerge anim import --hero yamato --slot reload_idle --from my_taunt.glb \
       --encode-vpk taunt_dir.vpk
   ```

Then install in `game/citadel/addons/` (or let Grimoire number it) and trigger the
slot in-game.

## Built vs. missing

**Built and proven:** the quantizer (`encode_compressed_pose`), the byte-faithful
blob splice (`patch_kv3_resource_blob`), glTF export + preview (`nm_clip_to_clip`,
`to_glb`), and in-game acceptance of re-encoded clips (single-hue, bow, spin,
moonwalk, slow-mo all confirmed). The codec round-trips (decode -> encode -> decode
pose-identical, 90%+ byte-exact).

**Arbitrary-length blob write — DONE (2026-06-14).** `replace_blob_v5` (via
`kv3::set_sole_blob` / `morphic::patch_kv3_resource_sole_blob`) writes a pose blob
of any length up to one LZ4 frame (16 KB; ~70 frames of 29 channels, more for
fewer), updating `sizeBlobs`/`comp2`/`comp_total` + the per-blob length + the
frame-size table. Staying within one frame keeps buf2's uncompressed size fixed, so
no document-array reshaping. CI: `sole_blob_resize_round_trips`. `set_scalars` and
`set_bools` gained the blobbed-LZ4-v5 branch (decompress-working + reassemble) they
were missing, so offsets and flags can be patched on a clip.

**Encoder v1 — DONE, IN-GAME CONFIRMED (2026-06-14).**
`morphic::model::reencode_nm_clip(original, &NmClip)` re-encodes a clip with a
**changed animated-rotation channel set at a fixed frame count**: rotation tracks
may be added (a static bone becomes animated) and re-posed; it splices the (now
longer/shorter) stream, rewrites the per-frame offsets, and flips each
newly-animated bone's `m_bIsRotationStatic`. CI: `reencode_adds_a_rotation_channel`.
Verified in-engine: a re-encode that converted all 68 static rotation tracks of
Yamato's `reload_idle_quick` to animated loaded and played (whole upper body
wobbled). The container fix that unblocked it: a blobbed KV3 block has a second
`0xFFEEDD00` trailer after the compressed blob frames that the engine asserts on
(morphic's reader ignores it); `replace_blob_v5` now re-appends it. Validated
offline against VRF/Source2Viewer (the engine-grade KV3 reader) before in-game.

**Bone masks are a real constraint.** Only the **upper body** moved in that test:
the animgraph blends `reload_idle_quick` over an upper-body bone mask, so authored
leg/lower-body channels are discarded for that slot even though the clip carries
them. Pick the slot by which bones it un-masks: a full-body idle (e.g.
`hideout_stand_idle`) for whole-body motion, an upper-body slot for arm/torso work.

**Full encoder — DONE (CI; v4 path pending in-game confirm).**
`morphic::model::reencode_nm_clip_full(original, &NmClip)` closes both remaining
gaps by **rebuilding the whole DATA block from the value tree** (uncompressed v4 via
`encode_kv3_resource`) instead of patching in place. Each former blocker becomes a
plain tree edit: adding a **translation/scale channel** (its range is recomputed
from the channel's min/max and written as a real value -- the writer re-tags
everything, so no tagless-constant problem) and changing the **frame count** (the
`m_compressedPoseOffsets` array is rebuilt and `m_nNumFrames` set). It recomputes
every animated channel's range from data, so untouched channels re-quantize within a
sub-step (use the in-place `reencode_nm_clip` when byte-faithfulness matters). CI:
`full_reencode_adds_a_translation_channel`, `full_reencode_changes_frame_count`,
`full_v4_reencode_round_trips`. The identity v4 re-encode opens cleanly in
Source2Viewer (VRF); an *edited* v4 re-encode still needs an in-game confirmation
(v4-uncompressed clips are new in-engine territory; the v5 surgical path is already
confirmed).

So the encoder is feature-complete for the importer:
- **Resampling to the slot's clock** is now free either way -- match the frame count
  (in-place) or change it (full re-encode).
- **Engine timing** is clip-duration-driven (confirmed via the Warden slow-mo test),
  so a resampled clip plays at the slot's duration.

## De-risking order

Before the Blender importer, land the **full clip encoder** and prove it with a
byte-faithful round-trip test: take a real clip, **rebuild it from scratch** through
the new encoder (recompute static/animated flags + ranges + frames, write a fresh
blob of possibly different length), and confirm it re-decodes identically (or
pose-identically within quantization). Once that holds on the committed fixtures and
across a full pak, the Blender importer is a thin layer: glTF animation -> sample ->
`NmClip` -> the encoder.

## Grimoire integration (eventual)

Folds into the mod manager: pick a hero + slot, drop a `.glb`, and it shows the clip
**animating on the actual hero mesh in-browser** (three.js — why the GLB export and
the IBL/cubemap work matter) before committing; a cheap spring-sim on the cloth/hair
bones papers over the runtime-sim gap so the preview reads right; one click installs.
Same morphic core behind the CLI and Grimoire. Trust story: author visually, preview
on the real mesh, and the round-trip guarantee means what you preview is what ships.

## Constraints (hold regardless of tool)

- **Replace-only** — overwrite an existing slot; cannot register a new animation.
- **Same skeleton** — no cross-hero retarget without a bone retargeter (heroes have
  unique rigs, 98-126 bones).
- **Match the slot's additive type** — additive deltas and absolute poses are not
  interchangeable.
- **Cloth/hair bones are runtime-simulated**, never in clips, so they won't follow an
  authored pose (preview dodge: spring-sim them in three.js).
- **Online**: the server animates vanilla, so own-hero / cosmetic surfaces only.
