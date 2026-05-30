# Handoff: vertex-color recolor (Paige ult horse/knight)

Status: **DONE - in-game confirmed.** Paige's ult horse/knight render correctly and
read purple. Ships as `vpkmerge model recolor`. The two hard-won lessons (which model
actually renders, and how to edit a meshopt buffer the engine accepts) are in
[The working workflow](#the-working-workflow) and [Encoding: the hard part](#encoding-the-hard-part).

Background: picked up after the texture + particle recolor of Paige (bookworm) was
working in game for bullets + abilities 1/2/3, but her **ult (the horse/knight) was
still green**.

## The working workflow

The repeatable recipe for recoloring a vertex-colored ability model:

1. **Find the model that actually renders.** An ult's body is spawned by a model
   particle, so it is referenced by a `.vpcf_c`, not named obviously. Walk the
   hero's ability particles for `.vmdl` refs and match the ult:

   ```
   # (dev) which models do the bookworm ult particles reference?
   #   bookworm_ultimate_model.vpcf_c -> models/particle/bookworm_horse_knight.vmdl
   #   bookworm_melee_swing_heavy_model.vpcf_c -> models/particle/bookworm_mace.vmdl
   ```

   Paige's ult body is **`models/particle/bookworm_horse_knight.vmdl_c`** (material
   `bookworm_knight.vmat`, vertex-colored). The `heroes_wip/bookworm/bookworm_horse*`
   models are **not** spawned by the ult - editing them did nothing.

2. **Recolor + pack** (one addon, each model overrides its base entry in place):

   ```
   vpkmerge model recolor --vpk pak01_dir.vpk --hue 280 \
     --encode-vpk ultbody_hue280_dir.vpk \
     models/particle/bookworm_horse_knight.vmdl_c \
     models/particle/bookworm_mace.vmdl_c
   ```

   `--list` first to see each model's color buffers. The CLI handles both buffer
   encodings automatically (see below).

3. **Install + load order.** Drop the addon in `game/citadel/addons/` (Grimoire
   manages numbering; it may renumber/remove a manual pak on its next apply).
   Disable any other Paige skin (e.g. a blue reskin) so it does not fight the hue.

## Encoding: the hard part

Editing a model's bytes so the **engine** (not just morphic) accepts it took two
wrong turns; both are now handled by `vpkmerge model recolor`:

- **Don't rebuild the container when you don't have to.** The first attempt ran an
  uncompressed buffer through `rebuild_with_block`, which re-pads to 16-byte
  alignment and shifts blocks (+11 bytes). The fix: for an **uncompressed** buffer,
  patch the `COLOR` bytes *in place* in the file - the output is byte-identical
  except the color lane (a real recolor changes exactly `vertices x 3` bytes).
- **Don't re-encode meshopt.** morphic's meshopt vertex encoder emits codec v1
  all-literal; Valve wrote `models/particle/*` as v0, and the re-encoded stream is
  mis-decoded by the engine (renders garbled). The fix: for a **meshopt** buffer,
  decode it, edit the color, and store it back **uncompressed**, flipping
  `m_bMeshoptCompressed` to false in the `CTRL` registry byte-faithfully
  (`morphic::kv3::set_bools`). The engine reads uncompressed buffers natively (the
  hero models ship them), so no meshopt encoder is involved. Geometry stays
  byte-identical; only the color changes.

Why not a material edit (like the blue reskin's `bookworm_dragon.vmat`)? The ult
body's material `bookworm_knight.vmat` paints vertex colors with
`g_bApplyTintToVertexColors = 0`, so a tint cannot reach them. Vertex colors are
the only lever, which is why the model edit is required.

## The finding (why the ult isn't recolored)

Paige's ult summons a horse + knight **model**. Its green is baked into the **mesh vertex
colors** - a third color mechanism, distinct from the two we already handle:

1. Particles (`.vpcf_c` color params) - done via `pak02` (the user's purple particle recolor, mean hue 280).
2. Model/self-illum **textures** (`.vtex_c`) - done via `pak04` (this session, 9 textures, hue 280).
3. **Mesh vertex colors** - NOT done. This is the ult horse.

Evidence (conclusive by elimination):
- The ult's particle glow IS purple: `pak02` has 34 `bookworm*ultimate*` + 53 `bookworm*dragon*` `.vpcf_c`.
- The horse/knight is a model, and there are **zero** `horse`/`knight`/`mace` particles in the base pak.
- Material `models/heroes_wip/bookworm/materials/bookworm_knight.vmat_c`: gray albedo (`g_tColor` is a 4x4 gray swatch, sat 0), **white** tints (`g_vColorTint1 = [1,1,1]`, `g_vSelfIllumTint1 = [1,1,1]`), and crucially **`F_PAINT_VERTEX_COLORS = 1`** + `g_bApplyTintToVertexColors = 1`, `g_fVertexColorStrength1`.
- No colored horse/knight texture exists (searched horse/knight/mace/excalibur/lance; only the gray knight albedo + masks).
- A material tint can't fix it: tint multiplies, so it cannot turn green into purple. The only fix is editing the per-vertex COLOR attribute in the mesh.

Target models (recolor all three):
- `models/heroes_wip/bookworm/bookworm_horse.vmdl_c`
- `models/heroes_wip/bookworm/bookworm_horse_knight.vmdl_c`
- `models/particle/bookworm_horse_knight.vmdl_c`
(plus possibly `models/particle/bookworm_mace.vmdl_c` - check if it carries color)

## What's already built (reuse these)

- **`vpkmerge texture`** (CLI) + **`vpkmerge_core::recolor`**: in-place `.vtex_c` hue recolor
  (`recolor_texture_hue` / `_image` / `_preview_png`, `inspect_texture`, `read_vpk_entry`).
  Now takes **multiple entries -> one addon**. Tests in `vpkmerge-core/src/recolor.rs`.
  The HSV `set_hue(rgb, hue_deg)` (set absolute hue, keep S+V) is the color transform to reuse.
- **Model edit pipeline** (`morphic::model`, see `docs/handoff-model-edit.md`): the proven
  in-place vertex-buffer edit + repack used for vertex displacement. Vertex color is just
  another interleaved attribute, so this is the machinery to extend.
- Installed addons (Deadlock `game/citadel/addons/`): `pak02` (particles) + `pak04` (9 textures),
  both hue 280. `pak04` source: `.scratch/paige_vfx_textures_hue280_dir.vpk`. The 9 textures:
  projectile self-illum, aoe_ground_projected, ground_streak, ui_effects, shield/sword/stone
  `_illustrated_color`, dragon_color, neutral_black_dragon_color (full table in
  `../grimoire/docs/ability-vfx-recolor.md`).

## Implementation plan (morphic model-buffer edit)

Reuse the geometry-edit pipeline; only the attribute being written changes (COLOR vs POSITION).

1. **`morphic` `OnDiskBuffer`** (`morphic/src/model/vbib.rs`):
   - `vector4(attr) -> Vec<[f32;4]>` already READS a 4-component COLOR (vbib.rs:296).
   - `write_positions(&mut self, ...)` (vbib.rs:242) is the template for an in-place interleaved
     overwrite (touches only that attribute's lane in the stride; rejects count mismatch).
   - ADD `write_colors(&mut self, attr: &InputLayoutField, colors: &[[f32;4]])` mirroring it,
     handling the COLOR field's `DxgiFormat` (most likely `R8G8B8A8_UNORM`; also cover the
     half/unorm variants `vector4` reads). Unit-test round-trip like `write_positions_*` tests.
   - Find the attribute via `field(|f| f.semantic_name == "COLOR")`.

2. **`morphic` model edit** (`morphic/src/model/edit.rs`, where `write_positions` is called at edit.rs:361):
   - ADD a recolor path mirroring `edit_model_geometry` / `reencode_model_mdat`: decode each
     vertex buffer (`BufferDesc::decode`), for buffers with a COLOR semantic read `vector4`,
     hue-shift each color (reuse the HSV transform), `write_colors`, then re-encode the mdat
     and rebuild the `.vmdl_c`. Recolor ALL COLOR attributes across all mesh parts/buffers.

3. **`vpkmerge_core`**: expose `recolor_model_vertex_colors(vmdl_bytes, hue) -> Vec<u8>`
   (in `model.rs` or `recolor.rs`).

4. **CLI**: add a `model ... recolor` action (or a top-level verb) mirroring the `texture`
   subcommand: `--from-vpk <vpk> --hue <deg> --encode-vpk <out>`, multi-entry -> one addon.

## Risks / resolve-first

- **CONFIRM the COLOR attribute exists and is green.** `mod vbib;` is private (not `pub`), so
  add a temporary debug (or a model-inspect that lists `semantic_name`s + samples `vector4`)
  on `bookworm_horse_knight.vmdl_c`. This is step 1 - don't build on the assumption.
- COLOR on-disk format (confirm `R8G8B8A8_UNORM`); mirror whatever `vector4` decodes for it.
- meshopt re-encode round-trip for these specific models (positions are proven; color is the
  same buffer, so it should hold - verify decode(encode(decode)) is stable).
- Color space: the texture/particle recolor operated on display-space 8-bit. Vertex colors may
  be linear; eyeball the result and adjust if hue lands wrong.
- A bad model edit can crash the game on load - verify the repacked `.vmdl_c` re-decodes before installing.

## What got built

The build matched the plan above. Net new public API:

- **`morphic`** (`model/vbib.rs`): `OnDiskBuffer::write_colors(attr, &[[f32;4]])`
  (in-place COLOR-lane overwrite, mirrors `write_positions`; handles
  `R8G8B8A8_UNORM` + the half/float variants `vector4` reads) and
  `color_fields()`. (`model/edit.rs`): `read_vertex_colors(vmdl, block) ->
  Option<Vec<[f32;4]>>` (diagnostic read), `recolor_vertex_buffer(vmdl, block,
  transform) -> (Vec<u8>, lanes)` (read every COLOR lane, apply the transform,
  re-encode, splice), and `VertexTarget::has_color`. **Handles both buffer
  encodings**: meshopt buffers go back through the vertex codec; uncompressed
  buffers get the COLOR lane patched in place (preserving padding).
- **`vpkmerge_core`** (`recolor.rs`): `recolor_model_vertex_colors(bytes, hue) ->
  (Vec<u8>, ModelRecolorStats)`, reusing the exact texture/particle `set_hue`
  (8-bit display-space) so one hue value lands models + textures + particles on
  the same color. (`model.rs`): `recolor_models_to_addon(vpk, entries, base, hue,
  out)` (multi-model -> one addon, mirrors the `texture` batch path).
- **CLI**: `vpkmerge model recolor [--list] --vpk <vpk> [--base <vpk>] --hue
  <DEG> --encode-vpk <OUT_dir.vpk> <ENTRY>...`.

Build command for the ult set (hue 280), source addon in
`.scratch/bookworm_horse_vcolor_hue280_dir.vpk`:

```
vpkmerge model recolor --vpk pak01_dir.vpk --hue 280 \
  --encode-vpk bookworm_horse_vcolor_hue280_dir.vpk \
  models/heroes_wip/bookworm/bookworm_horse.vmdl_c \
  models/heroes_wip/bookworm/bookworm_horse_knight.vmdl_c \
  models/particle/bookworm_horse_knight.vmdl_c \
  models/particle/bookworm_mace.vmdl_c
```

### Findings from the diagnostic (step 1, confirmed)

- **The COLOR attribute exists and is green on every horse/knight model**, mean
  hue ~136, 100% of vertices, `R8G8B8A8_UNORM` (e.g. `bookworm_horse` reads
  `[128,252,162,255]`). Format confirmed; the recolor read/write path is
  format-faithful either way.
- **The mace DOES carry green vertex color** (the doc's open question): include
  it. So the ult set is **four** models, not three.
- **Buffer layout differs by model class**: the two hero models
  (`bookworm_horse`, `bookworm_horse_knight`) carry COLOR in a standalone
  **uncompressed** buffer (block 1, stride 24, which also holds POSITION); the two
  particle models (`particle/bookworm_horse_knight`, `particle/bookworm_mace`)
  carry it in a **meshopt** interleaved buffer (block 0, stride 32). Both paths
  are handled and tested.
- **`bookworm_dragon` and `bookworm_sword` ALSO carry green vertex colors** (hue
  ~136). They're likely other abilities (the texture recolor already covered
  dragon/sword textures in `pak04`). Not in the shipped ult set; recolor them too
  if the ult verifies purple and they still read green in game (`vpkmerge model
  recolor ... bookworm_dragon.vmdl_c bookworm_sword.vmdl_c`).
- `bookworm_ui_effect` is neutral (sat 0, correctly a no-op under hue-set);
  `bookworm_shape.vmdl_c` fails to decode ("unsupported embedded-mesh layout") and
  is not a target.

### Offline verification (done)

- `morphic` unit tests: `write_colors` lane isolation + count-mismatch; gated
  `recolor_vertex_colors_round_trips_local` (in `tests/model_local.rs`, run with
  `MORPHIC_MODEL_VPK=<pak01>`) proves identity recolor is lossless and a
  channel-swap applies, with positions byte-identical, on **both** buffer paths.
- End-to-end on the real ult set: all 4 models re-decode after recolor; colors
  move to mean hue ~280 (green 0%); saturation/value unchanged; and a positions
  diff (base vs addon) is byte-identical (max |d| = 0) for every buffer, so
  geometry was not touched. (Diagnostics: `vpkmerge-core/examples/vertexcolors.rs`
  + `poscheck.rs`, throwaway.)

> Note: the "What got built" / findings above were written during the build, when
> the target was assumed to be the `heroes_wip/bookworm/bookworm_horse*` models.
> In-game testing then showed those are **not** what the ult renders - the real
> body is `models/particle/bookworm_horse_knight.vmdl_c` (see
> [The working workflow](#the-working-workflow)). The 8-bit display-space hue was
> correct (no linear-space adjustment needed): hue 280 read purple in game.

## Outcome

In-game confirmed: the ult horse/knight render correctly and read **purple**. The
color space did not need adjusting (8-bit display hue matched the textures/particles).

### Checklist (all done)

1. [x] Confirm COLOR vertex attribute + format on the models.
2. [x] `morphic`: `OnDiskBuffer::write_colors` + unit round-trip test.
3. [x] `morphic`: model-level recolor pipeline + stability test (both buffer encodings).
4. [x] `vpkmerge_core` + CLI exposure (mirror `texture`).
5. [x] Found the real ult body model, recolored it (meshopt -> uncompressed), packed, **in-game confirmed purple**.
6. [x] Update `../grimoire/docs/ability-vfx-recolor.md` + `CLAUDE.md` + this doc.

### Possible follow-ups

- `models/particle/bookworm_mace.vmdl_c` (melee swing) is recolored in the same
  command. `bookworm_dragon` / `bookworm_sword` carry green vertex colors too
  (other abilities) - add them to the recolor if they read green in game.
- The throwaway dev examples used to find all this (`vpkmerge-core/examples/*.rs`:
  `vertexcolors`, `poscheck`, `modelrefs`, `blockver`, `vmatdump`, ...) were not
  committed; recreate as needed.

## Diagnostics (throwaway `vpkmerge-core/examples/*.rs` were deleted; recreate as needed)

- Base pak: `~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`.
- List models: `valve_pak::open(pak).file_paths()` filtered to `.vmdl_c` containing `bookworm`.
- Material color params: `morphic::decode_kv3_resource` on a `.vmat_c`, walk for
  `m_vectorParams[*]/m_name` (`g_vColorTint1`, `F_PAINT_VERTEX_COLORS`, ...) + `m_value`.
- Texture chroma: `morphic::decode` -> mean saturation/hue over opaque pixels (color-bearing if sat high).
- Particle hue: `morphic::decode_kv3_resource` on `.vpcf_c`, walk `color`/`tint`-keyed int arrays.
