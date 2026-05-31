# Spike: in-place recolor of a BLOBBED Source 2 material (`.vmat_c`)

Status: **Mechanism landed + verified (unit + strict VRF load). In-game gate
pending for the user.** Blobbed LZ4 v5 materials now retint in place and re-emit
in the engine's native compressed format. The two named targets
(`necro_hands.vmat_c`, `inferno_body.vmat_c`) both patch and pass a strict
ValveResourceFormat load with correct recolored values.

## The problem

Some hero ability materials carry their color in a `g_vColorTint` /
`g_vSelfIllumTint` constant (an RGBA `f64` vector in the `.vmat_c` KV3 `DATA`
block), not in a texture. `vpkmerge_core::recolor_material_color_bytes` retints
those in place via `morphic::patch_kv3_resource_doubles` (= `kv3::set_doubles`),
which decompresses the KV3 block, patches the `f64`s, and rebuilds the resource
envelope, leaving everything else byte-faithful.

That worked for materials whose `DATA` block has **no** binary-blob section
(`countBlocks == 0`). It failed for **blobbed** materials (`countBlocks > 0`): the
old fix decompressed the whole block and flipped `compressionMethod` to `0`, which
left a now-stale per-frame size table in the buffer tail. morphic's own (lenient)
reader ignores that table when `comp == 0`, so a decode round-trip **falsely
passed**; but the engine still consults it, misreads the blob, and renders the
covered mesh as a **flat-red error material** in-game (observed on Infernus's upper
body; `necro_hands` behaves the same). So `rewrap_uncompressed` was made to
**refuse** any blob section, and blobbed materials were left vanilla.

The two concrete targets:

- `models/abilities/materials/necro_hands.vmat_c` (Graves' wall-of-hands energy)
- `models/heroes_staging/inferno_v4/materials/inferno_body.vmat_c`

Both are KV3 v5, `compressionMethod = 1` (LZ4), `countBlocks = 1`.

## KV3 v5 binary layout (block-relative offsets)

A compiled KV3 `DATA` block is a 120-byte header followed by two LZ4 (or ZSTD)
buffers and, when `countBlocks > 0`, a binary-blob frame region.

```
off  field
  0  magic 0x4B563305 (low byte = version 5)
 20  compressionMethod   0 = none, 1 = LZ4, 2 = ZSTD
 26  u16 LZ4 blob frame size (16384)
 40  countTypes
 44  u16 countObjects   46  u16 countArrays
 48  sizeUncTotal       = unc1 + unc2          (blobs NOT counted)
 52  sizeCompTotal      = comp1 + comp2         (blob frames NOT counted)
 56  countBlocks        (blob count)
 60  sizeBlobs          (uncompressed total of all blobs)
 64  count_b2 (aux)     68  sizeBlockCompressed (LZ4 per-frame table size, bytes)
 72  unc1  76  comp1     aux buffer (strings + primitive arrays) uncompressed/compressed
 80  unc2  84  comp2     main buffer (object lengths, scalars, type stream)
 88/92/96/100  main b1/b2/b4/b8 lane counts
108  main object-length count
120  header end
```

Body layout, in stream order:

```
[header: 120][buf1: comp1][buf2: comp2][blob frames: rest]
        buf1 starts at 120
        buf2 starts at 120 + comp1
        blob frames start at 120 + comp1 + comp2
```

There is **no absolute offset stored anywhere** for the blob frames: the engine
reads buf1 (consuming `comp1` bytes), then buf2 (`comp2` bytes), then the blob
frames from wherever the stream now sits. Everything after buf1 is located by the
buffer **size** fields, sequentially.

`buf2`'s **tail** (after the type stream) carries the blob bookkeeping, in the
*uncompressed* bytes:

```
[per-blob uncompressed lengths: countBlocks * i32]
[4-byte document trailer 0xFFEEDD00]
[LZ4 per-frame compressed-size table: sizeBlockCompressed bytes of u16]   (LZ4 only)
```

The compressed blob **frames** themselves live in the trailing region after buf2.

### comp = 1 (LZ4, the shipped form) vs comp = 0 (the broken re-emit)

| aspect | comp = 1 (native) | comp = 0 (broken old fix) |
|---|---|---|
| buf1/buf2 | one LZ4 block each | raw bytes, `comp* = unc*` |
| blob frames | LZ4 frames; sizes in buf2-tail table | the old fix appended raw blob bytes |
| frame table | live, consulted by the engine | **stale**, left in buf2 tail; `sizeBlockCompressed` zeroed but the table bytes remain |
| morphic reader | reads it | tolerates it (branches on `comp`, skips the table) -> false pass |
| engine / strict VRF | reads it | **misreads**: a strict VRF load throws `Assertion 'trailer == 0xFFEEDD00' failed`; the engine shows a red error material |

The comp = 0 path is a structural rewrite the engine does not tolerate. The fix is
to **not rewrite the structure at all**.

## The fix (Approach A): keep `comp = 1`, recompress only the buffer that changed

Where the tint doubles actually live matters. The RGBA `m_value` array is encoded
as an `ARRAY_TYPE_AUXILIARY_BUFFER`, so its `f64`s sit in **buf1 (the aux
buffer)**, not buf2. (The task brief assumed buf2; the real files put them in
buf1. The implementation recompresses whichever buffer changed, so it is correct
either way.)

Steps, in `morphic::kv3::{rewrap,patch}`:

1. `is_blobbed_lz4_v5(block)` -> a v5, `comp == 1`, `countBlocks != 0` block takes
   the blob path (ZSTD-blobbed blocks are excluded: we have no ZSTD encoder, so
   they still hit the refusal and are left vanilla).
2. `decompress_v5_working(block)` -> a flat, walkable uncompressed copy
   `[original 120-byte header][raw buf1][raw buf2]`. The blob frames are omitted: a
   `BINARY_BLOB` node consumes no typed-lane bytes and the walker never reads past
   buf2's type stream, so the two decompressed typed buffers are all it needs. The
   header's `unc1` (offset 72) still locates buf2 at `120 + unc1`, exactly as the
   lane walker expects.
3. `set_doubles` runs its normal `PathWalk` over the working copy and patches the
   target `f64`s in place (identical contract to the non-blob path).
4. `reassemble_blobbed_v5(orig, patched_working)` re-emits a `comp = 1` block:
   - Recompress **only** the typed buffer whose raw bytes changed (here buf1), via
     `lz4_flex::block::compress` (already a dependency; no new crate, no encoder to
     write). The unchanged buffer is spliced through byte-for-byte.
   - The entire blob frame region is spliced through **byte-for-byte**: the
     per-blob length table, the trailer, the per-frame size table, and the frames
     are all untouched by a tint edit, so they stay valid.
   - Rewrite the three size fields that move: `comp1`(76), `comp2`(84), and
     `sizeCompTotal`(52) ` = comp1 + comp2`. `sizeUncTotal`(48), `countBlocks`(56),
     `sizeBlobs`(60), `sizeBlockCompressed`(68), the frame size(26), and
     `compressionMethod`(20) are all unchanged.
   - Because the blob frames are located by the (now-updated) size fields, they
     relocate correctly with no other change.
5. The resource envelope is rebuilt by `Resource::rebuild_with_data` (unchanged).

`lz4_flex`'s compressed block is a standard LZ4 block, decodable by the engine's
LZ4 decoder; the recompressed buffer is typically a few dozen bytes larger than
Valve's (different encoder), which is why a patched file grows slightly.

This is strictly smaller and safer than the comp = 0 approach: the output is the
engine's **native** format, differing from the original only in (a) buf1's LZ4
byte stream and (b) the patched 8-byte doubles inside it. Nothing about the blob
framing or the compression method changes, so it does not rely on any engine
tolerance the comp = 0 form needed.

## Verification

1. **Unit (`morphic/src/kv3/patch.rs`, `morphic/tests/kv3.rs`):** patch a tint
   channel of the committed real fixture
   `morphic/fixtures/material/necro_hands.vmat_c`; assert the output stays v5 +
   `comp = 1` + `countBlocks = 1` (NOT flipped to comp = 0), `buf2` and the blob
   frames are byte-identical, the size fields stay consistent, and a full-tree
   re-decode shows **only** the targeted channel changed (every sibling, including
   the binary blob, is unchanged). morphic re-decode alone is a known false
   positive, so this is necessary but not sufficient.
2. **Strict VRF load (the oracle):** `tools/morphic-oracle validate --file PATH`
   does a strict ValveResourceFormat `Resource.Read` and fully materializes the
   material (forcing the KV3 blob section to parse). Results:
   - Original `necro_hands` / `inferno_body`: load OK (sanity).
   - **Approach-A recolored** `necro_hands`: loads OK, `g_vColorTint1 = (0.682,
     0.149, 0.949, 1)` and `g_vSelfIllumTint1 = (0.706, 0.129, 0.996, 1)` (hue
     280, purple). `inferno_body`: loads OK, `g_vSelfIllumTint1 = (0.627, 0.071,
     0.906, 0)`.
   - **Old comp = 0 form** (reconstructed): **REJECTED**, `UnexpectedMagicException:
     Assertion 'trailer == 0xFFEEDD00' failed`. This proves the VRF gate
     discriminates the broken form from the good one (morphic's reader does not).
   - End-to-end: the `necro_hands` entry extracted from the actual baked addon VPK
     (`vpkmerge recolor-hero --hero necro ...`) also passes the strict load.
3. **In-game (the real gate, for the user to run):** see below.

## In-game test (pending)

morphic re-decode and even a strict VRF load are not the engine. The definitive
check is the live game.

```bash
PAK=~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk
# Bake the whole Graves ability-VFX recolor (now includes necro_hands):
cargo run -p vpkmerge-cli -- recolor-hero --hero necro --vpk "$PAK" --hue 280 \
  --encode-vpk /tmp/necro_recolor_dir.vpk
# Install into a free addon slot (03, 08, or 11):
cp /tmp/necro_recolor_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak08_dir.vpk
```

Launch Deadlock, play Graves, trigger the wall-of-hands ability, and confirm the
raised hand reads **recolored (purple)**, not green (un-recolored), and crucially
**not red / wireframe** (the broken-material signature). To test `necro_hands` in
isolation, pack just that one recolored entry into a one-entry addon
(`vpkmerge_core::pack`) at its base path instead of the full bake.

## Files touched

- `morphic/src/kv3/rewrap.rs`: `is_blobbed_lz4_v5`, `decompress_v5_working`,
  `reassemble_blobbed_v5`, `recompress_if_changed`. `rewrap_uncompressed` still
  refuses blobs (it protects the scalar/bool/draw-call callers that ship comp = 0).
- `morphic/src/kv3/patch.rs`: `set_doubles` routes a blobbed LZ4 v5 block through
  the decompress -> walk/patch -> reassemble path; everything else is unchanged.
- `morphic/fixtures/material/necro_hands.vmat_c`: committed blobbed fixture.
- `tools/morphic-oracle`: `validate --file` subcommand (strict loose-file load).
- `vpkmerge-core/src/hero_recolor.rs`: comments/report wording updated;
  `necro_hands` (already in Graves' recipe) now recolors instead of being skipped.

---

# Follow-up: matching the reference recolor mods (Graves held flame + aura)

After the blob re-emit landed, comparing our recolor against two known-good
hand-made mods (`pink_graves_abillities_and_affects`, `blue_infernus_particles_commission`)
surfaced three gaps that left Graves' **held flaming-hand prop and its aura**
un-recolored. All three are now fixed.

## What the reference mods do

A reference recolor edits three things, and the bulk of the held-flame color came
from edits our pipeline didn't make:

1. **Particles** under `abilities/`, `weapon_fx/`, **and `heroes/`**. Our prefixes
   missed `particles/heroes/necro/` (the held-weapon ambient flame), so that flame
   stayed green. Added the prefix.
2. **Effect-material tints**, **stamped with one absolute brand color** (deep pink
   `[1.0, 0.078, 0.576]` = RGB 255,20,147) on `g_vColorTint` / `g_vSelfIllumTint`,
   *regardless of the original* -- including overriding a **white** tint (the
   additive `picker_hand_glow` aura, vanilla `[1,1,1]`).
3. Color textures (the bony hand albedo + transmissive glows), which we already
   covered. The picker-hand effect's own `g_tColor` is a shared grayscale grunge
   texture, so its color is the tint, not the map.

## Gap 1 -- absolute stamp vs hue-preserve

Our recolor *sets the hue while keeping each color's saturation*, so a **white**
tint (zero saturation) stays white -- the `picker_hand_glow` aura never colorized.
The reference instead **stamps** a flat brand color. `recolor_material_color_bytes`
now stamps (`crate::recolor::stamp_rgb`: the picked hue at full saturation/value),
with two write paths so the result stays engine-loadable:

- **Stored-double channels** (a colored tint, or a white tint stored as real
  doubles) -> byte-faithful in-place double patch (handles blobbed materials).
- **Tagless `DOUBLE_ZERO`/`DOUBLE_ONE` channels** (a neutral 0.0/1.0 with no stored
  bytes, e.g. the additive glow's white `g_vColorTint`) -> the channel can't be
  patched in place, so the **whole non-blobbed material is re-encoded**
  (`encode_kv3_resource`), promoting it to a real double. Verified safe for
  materials: VRF strict-loads the result and the texture params resolve
  identically (the re-encode preserves the texture **RERL/RED2** dependency blocks
  byte-for-byte; only models, which lean on auxiliary-typed-array tags, can't
  survive a re-encode). The fallback refuses a **blobbed** material (a re-encode
  emits blobs uncompressed, which the engine misreads).

## Gap 2 -- a second blob-frame reader bug (`countBlocks == 2`)

The held-hand's main material, `necro_picker_hand_effect.vmat_c`, carries **two**
binary blobs (its `m_dynamicParams` expressions) and morphic couldn't even decode
it: `blob frame: expected 12, got 6`. The blob-frame decoder assumed each LZ4 frame
fills the whole remaining region, but multiple small blobs are framed
**one-per-blob** (two 6-byte frames, not one 12-byte frame). `decompress_blob_frames`
now decodes each frame into the remaining buffer capped at `frame_size` and takes
whatever it yields, validating only the total. (Distinct from the re-emit fix
above; this is the *reader*.)

## Gap 3 -- recipe coverage

The Graves (`necro`) recipe gained the `particles/heroes/necro/` prefix and three
effect materials: `necro_picker_hand_effect` (the held hand), `necro_picker_effect`,
and `picker_hand_glow` (the aura). Bake report: 185 particles, 9 textures, **8/8
material tints stamped (0 unpatchable)**. All three new materials VRF-strict-load
as the brand color, both as loose files and as extracted from the baked addon.

## Infernus

Per the same comparison, `inferno_body.vmat_c` was **removed** from the recipe: the
reference blue mod does not touch it (the body tint is not what colors his fire).
The reference recolors the fire **ramp / burning / lava** textures, but those are
**hero-specific copies it repoints the particles to** -- the vanilla fire textures
are shared game-wide (the inferno flame particles sample `noise_flame_warp`,
`noise_voronoi_tiled`, `mask_vignette`, ...), so recoloring them in place would tint
*everyone's* fire. Hero-isolated fire-texture recolor needs a **rename + repoint**
step (recolor to a new path, then rewrite the particle's texture reference) that is
not built yet. Until then Infernus is recolored by particle params alone: his fire
reads the picked hue but keeps the vanilla texture's luminance.

## Verification (this follow-up)

- Unit: `decodes_a_two_blob_material` (reader fix); `stamps_an_absolute_brand_color_on_neutral_and_blobbed_tints`
  (neutral white -> re-encode, blobbed colored -> in-place, two-blob -> in-place).
- Strict VRF: `picker_hand_glow` (white -> pink, re-encode), `necro_picker_effect`,
  and `necro_picker_hand_effect` all load as `(1, 0, 0.53)` both loose and from the
  baked `recolor-hero --hero necro` addon.
- In-game (the user's gate, still pending): play Graves, raise the soul-picker hand,
  confirm the held flame **and its aura** read the picked color (not green/white).

## Files touched (follow-up)

- `morphic/src/kv3/reader.rs`: `decompress_blob_frames` multi-blob fix.
- `morphic/src/lib.rs`: `kv3_resource_has_blobs` (guards the re-encode fallback).
- `vpkmerge-core/src/recolor.rs`: `stamp_rgb`.
- `vpkmerge-core/src/hero_recolor.rs`: `recolor_material_color_bytes` stamps
  (in-place + re-encode fallback); necro recipe +1 prefix +3 materials; inferno
  recipe drops the body material.
- `morphic/fixtures/material/{necro_picker_hand_effect,picker_hand_glow}.vmat_c`.
- `tools/morphic-oracle`: `validate` now also dumps texture params.
