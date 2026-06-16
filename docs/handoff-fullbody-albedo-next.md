# Handoff: full-body hero VFX via the albedo texture path (+ what's next)

Date: 2026-06-17. Status: **research + direction; no code written for the albedo
pivot yet.** Prior work (blob-expr-replace, GLB mesh-group filter, FeModel JSON)
committed on branch `fix/glb-pure-normal-discriminator` (`9560c59`, `cf6a92d`,
`46f91f3`).

## Why this doc exists

A long session of making "wild" in-game hero effects via `.vmat_c` shader-param
edits (rainbow self-illum, strobe, glass/pbr/unlit presets) established a hard
ceiling, then pointed at the real path. This captures both so the next session
doesn't re-walk it.

## The finding that forces the pivot: every hero color lever is mask-gated

Deadlock heroes render through `pbr.vfx` with `F_USE_NPR_LIGHTING`. **Every**
color/emissive param is gated by a per-material texture mask:
- tint -> `g_tTintMaskRimLightMask`
- self-illum -> `g_tSelfIllumMask`
- outline -> `g_tNprOutlineMask` (and the `F_OVERRIDE_NPR_OUTLINE` override
  renders an *inward* shell, not a fat silhouette)

So a `vmat --set-expr` only paints the masked region. In-game this read as: great
on emissive-rich heroes (Infernus flames), and "just the iris / parts of the feet"
on heroes whose emissive mask is small (Doorman, Dynamo). **You cannot get a
uniform full-body effect from a shader param.** The `pbr`/`glass`/`unlit` presets
are full-surface but flopped for other reasons (pbr only looks shiny with a bright
environment; glass reads as partial translucent zones; unlit/gem too subtle).
See memory `npr-shader-mask-gating`.

**Conclusion: full-body, obvious, any-hero => repaint the albedo TEXTURE
(`g_tColor`), not a shader param.** The albedo is the whole surface, unmasked.

## What already exists for the texture path (don't rebuild)

- `vpkmerge_core::recolor::recolor_texture_hue` (CLI `texture --hue`): hue-SET
  keeping saturation+value. **Weak for full-body**: low-saturation skin/cloth
  barely moves (the documented caveat). Fine for already-saturated VFX textures,
  not for an obvious hero recolor.
- `vpkmerge_core::trippy` (CLI `trippy-skin`): the real tool. Generates seamless
  procedural albedo (14 styles: confetti, liquid, moire, kaleido, holo, glitch,
  thermal, gradient, camo, carbon, galaxy, halftone, lava, vaporwave), packs it at
  the hero's existing texture paths, AND byte-patches the material's scroll vectors
  so the paint flows at runtime. This is full-surface + animated -- exactly the
  "wild full-body" goal. `trippy_skin_to_addon(vpk, base, codename, opts, out)`.

## THE GAP (this is the next work): discovery targets dead models

Both `trippy::discover_targets` and `hero_recolor`'s discovery find materials by
**path-name heuristic** (`hero_path_match(path, codename)` over
`models/heroes*/.../*.vmat_c`). That is the exact mistake that wasted most of this
session:

1. **Staging vs live.** `heroes_staging/` and `_v2/_v3/_v4` dirs are full of DEAD
   models. The live model for a hero is `scripts/heroes.vdata_c ->
   hero_<codename>.m_strModelName` (e.g. Lady Geist `hero_ghost` =
   `models/heroes_wip/geist/geist.vmdl`, NOT `heroes_staging/ghost`). Editing the
   staging material shows nothing in-game.
2. **codename != model/path name.** `ghost` (Geist) -> `geist.*`, `atlas` (Abrams)
   -> `abrams.*`, `synth` (Pocket) -> `pocket.*`, `orion` -> `archer.*`. A codename
   substring match misses these entirely.
3. **Materials are not in the model's dir.** The live `heroes_wip/inferno` model
   renders `heroes_staging/inferno_v4/*` materials. Only the model's own draw
   calls know the truth.

### The fix (authoritative resolution -- already have the library pieces)

For a codename, resolve the real albedo textures like this:
1. Decode `scripts/heroes.vdata_c`; read `hero_<codename>.m_strModelName` (filter
   `m_bPlayerSelectable == true` for real heroes).
2. `vpkmerge_core::model_draw_call_targets` (re-exported; wraps
   `morphic::model::draw_call_targets`) on the live `.vmdl_c` -> the actually-
   rendered materials, with vertex counts. Biggest non-weapon = body.
3. For each rendered body material, read `g_tColor` (base color texture) ->
   `.vtex_c` entry. That is the texture to repaint/pack.

Wire that into `trippy::discover_targets` (and `hero_recolor::recipe_for`
discovery) in place of `hero_path_match`. Keep the existing repaint/scroll-patch
machinery unchanged -- only the target list changes. Add a `--list` that prints
the resolved (hero -> model -> body material -> g_tColor texture) chain so it's
verifiable before packing.

Throwaway recon tools built this session that already do steps 1-2 (in
`vpkmerge-core/examples/`, untracked): `hero_models.rs` (codename -> model +
selectable), `model_mats.rs` (model -> materials by vtx). `selfillum_candidates.rs`
and `blob_expr_census.rs` scan materials for self-illum / blobbed expressions.
Promote the resolution logic into the library (a `hero_live_body_textures(codename)
-> Vec<TextureTarget>` helper) rather than leaving it in examples. See memory
`hero-live-model-resolution`.

## Caveats to carry forward

- Hue-shift keeps saturation -> not obvious on desaturated albedo. For "obvious"
  prefer trippy style repaint (replaces the albedo) or add a saturation/brightness
  boost knob to the recolor path.
- `trippy-skin` already animates via scroll-vector patches; that is byte-faithful
  on non-blobbed materials. If a live body material is blob-bearing, the scroll
  patch must take the `replace_blob_v5` path (the just-committed blob-expr work) --
  verify which live body materials are `[dynamic]`.
- Addon mount: the game only loads `pakNN_dir.vpk` for NN in 00..=99 (NOT 100+).
  The user's `citadel/addons/` is managed by a `.dmm.json` mod manager; manual
  drops can hit load-order/cache flakiness.

## Current in-game state (installed test paks, all <= 99)

Hero rainbow/strobe showcase (mask-gated, minor coverage -- the motivation to move
to albedo): `pak80` Celeste, `pak81` Infernus, `pak82` Lady Geist, `pak83`
Familiar, `pak84` Paradox, `pak85` Dynamo, `pak86` Doorman, `pak87` Shiv, `pak88`
Apollo, `pak89` Calico, `pak90` Sinclair, `pak91` Paige. Plus `pak98` "wild map"
(souls/portals/objectives/shields strobe -- hero-independent, the most reliably
visible set). These are disposable demos; regenerate from the live-model targets
once discovery is fixed.

## Next steps, prioritized

1. **Library helper** `hero_live_body_textures(codename)` (vdata + draw_call_targets
   -> g_tColor `.vtex_c` list) + a CLI `--list` to verify the chain.
2. **Re-point `trippy-skin` discovery** at that helper; ship a full-body trippy
   skin on one live hero, confirm in-engine (this is the first real full-body win).
3. **Saturation/brightness knobs** on `recolor_texture_hue` for an obvious plain
   recolor without a full repaint.
4. **Audit `hero_recolor` recipes** against live models -- the pinned recipes were
   built before this resolution existed and may name dead entries.
5. Optional cleanup: commit or delete the untracked research examples; rename the
   branch (it now carries vmat + femodel + glb work under a stale glb name).
