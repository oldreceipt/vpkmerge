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
single-frame blob splice (`patch_kv3_resource_blob` -> `set_blob` ->
`replace_single_blob_v5`), glTF export + preview (`nm_clip_to_clip`, `to_glb`), and
in-game acceptance of re-encoded clips. The codec round-trips (decode -> encode ->
decode pose-identical, 90%+ byte-exact).

**Missing — two real pieces:**

1. **A full clip *encoder*, not a patcher.** Every edit so far kept the clip's exact
   shape (same frame count, same animated channels) so an *equal-length* blob could
   be spliced in place. A from-scratch Blender animation won't match: it may animate
   bones the slot left static, or want a different frame count. The importer must
   **build a fresh `NmClip`**: decide per bone which channels are animated, compute
   each channel's quantization range (min/max over the authored animation), write
   `m_nNumFrames` and the offset table, then write an **arbitrary-length** pose blob
   back into the container. That last step extends the single-frame blob writer to a
   size change: rebuild the LZ4 blob frame(s) + the per-frame size table in buf2 +
   the affected header sizes (today `replace_single_blob_v5` only handles an
   equal-length, single-frame swap).

2. **Resampling to the slot's clock.** The engine plays a slot at a fixed length
   (for abilities the duration is gameplay-timed), so the Blender timeline is
   resampled onto the slot's frame grid. Want it *longer* than the slot? Pick a
   longer slot (e.g. an idle), or have the encoder rewrite the frame count — the
   same full-rebuild path as (1). (This is the "reload is too short" problem: the
   reload slots are ~0.7 s.)

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
