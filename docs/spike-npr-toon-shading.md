# Spike: NPR / toon shading control via vpkmerge

Investigated 2026-06-09. Question: can vpkmerge expose Deadlock's NPR (toon)
shading as a moddable surface (outlines, cel banding, unlit looks) the way it
already exposes recolors, prisms, and trippy skins?

Short answer: **yes, and most of it is per-material `.vmat_c` data we can
already patch byte-faithfully.** Deadlock heroes are not PBR-with-a-painterly-
texture; they run a dedicated NPR path inside `pbr.vfx` that is *on by default*
(502 of 605 hero materials set `F_USE_NPR_LIGHTING=1`). "Adding toon shading"
therefore means *restyling* knobs the engine already honors, not bolting a new
shader on.

## 1. Evidence

### 1.1 The shader vocabulary (oracle `shader-dump`)

`shaders/vfx/pbr_pc_50_features.vcs` in `citadel/shaders_pc_dir.vpk` exposes a
full NPR parameter family. The `src=` class is the critical split:

**`__SetByArtist__` (lives in the `.vmat`, patchable by us):**

| Param | Meaning |
|---|---|
| `F_USE_NPR_LIGHTING` (SF) | the toon lighting path itself |
| `F_SOLID_COLOR_OUTLINE` (SF) | solid-color silhouette outline |
| `F_DISABLE_NPR_OUTLINE` (SF) | kill the outline for this material |
| `F_OVERRIDE_NPR_OUTLINE` (SF) | per-material outline thickness override |
| `F_UNLIT` (SF) | fully unlit (lighting ignored; albedo as-is) |
| `F_SHEEN` (SF) | sheen lobe |
| `g_vSolidOutlineTint`, `g_vSolidOutlineAdditive` | outline color (multiply + add) |
| `g_fSolidOutlineVertexColorTint` | how much vertex color tints the outline |
| `g_flOverrideNprOutlineThickness` (+`...Enemy`) | outline width, friendly/enemy variants |
| `TextureNprOutlineMask1` / `g_tNprOutlineMask` | where outlines appear |
| `TextureRimLightMask1` / `g_tTintMaskRimLightMask` | rim-light mask (G channel = rim constant; flat 4x4 on shipped skins) |
| `TextureNprTramsissiveColor1` / `g_tNprTransmissiveColor` | NPR transmissive color (Valve's typo) |
| Highlight Tint group (`g_vHighlightTint1`, `g_flHighlightCoverage1`, `g_flHighlightHardness1`, `g_flHighlightTintBrightness1`, `g_flInvertHighlight1`, `g_vHighlightPositionWs1`, `g_flHighlightRadius1`) | a positional stylized highlight |

**`__Attribute__` (engine-fed, NOT in any shipped material):** the actual
cel-band math: `g_flNPRDiffuseStepSharpness`, `g_flNPRDiffusePbrBlend`,
`g_flNPRDirectLightWrap`, `g_nNPRSpecularSteps`, `g_flNPRSpecularStepSharpness`,
`g_flNPRRimLightStrength/Falloff/Wrap`, `g_vNPROutlineBrightColor/DarkColor`,
`g_flNPROutlineThickness`, `g_vNPRLightWeights`, `g_vNPRExposureTargets`, etc.
A survey of all 605 hero `.vmat_c` files found **zero** float/vector attributes
on any material, so these are set globally by render code (per-mode/per-map),
not by content. Related convars in `client.dll`: `r_citadel_disable_npr_lighting`,
`r_citadel_npr_outlines`, `r_citadel_npr_outlines_max_dist`,
`r_citadel_npr_force_solid_outline`.

The engine also ships `generate_outlines` / `outline_buffer` (post-process
outline passes) and an `npr_dummy` shader. Environment shaders have **no** NPR
combos: this surface is hero/model-scoped, the world cannot be toon-ified by
flag-flipping.

### 1.2 What shipped content actually uses (`examples/npr_vmat_survey.rs`)

605 `.vmat_c` under `models/heroes*` in `citadel/pak01_dir.vpk`, all decoded:

| Flag/param | materials |
|---|---|
| `F_USE_NPR_LIGHTING` | 502 |
| `F_SELF_ILLUM` | 273 |
| `g_fSolidOutlineVertexColorTint` | 193 |
| `F_SOLID_COLOR_OUTLINE` | 151 |
| `F_SHEEN` | 26 |
| `F_DISABLE_NPR_OUTLINE` | 24 |
| `F_UNLIT` | 13 (e.g. `dynamo_void`, `hazev2_head`, `ivy_leaf`) |
| `F_OVERRIDE_NPR_OUTLINE` + thickness | 1 (`infernus_vertcol_trans`) |

Every mechanism we would expose is already exercised by Valve content, so the
engine demonstrably honors it as material data. Hero materials also carry
per-material outline colors today (Vindicta's dress: `g_vSolidOutlineTint`
dark red-brown), so outline restyling is a value edit, not a structural one.

## 2. What vpkmerge can ship, by tier

### Tier A: outline + flag restyle (pure vmat patch, machinery exists)

All edits ride the byte-faithful KV3 patcher (`morphic::kv3::patch`):
`patch_kv3_resource_doubles` for existing values (proven in-game by the trippy
scroll patches), `insert_array_element_adding` + `set_strings_adding` for
params a material does not yet carry (proven in-game by the animated prism).
**Never full-re-encode a `.vmat_c`** (same lesson as particles).

- **Outline recolor:** set `g_vSolidOutlineTint` / `g_vSolidOutlineAdditive`
  (white-ink sketch look, neon rims, complementary color).
- **Outline thickness:** add `F_OVERRIDE_NPR_OUTLINE=1` +
  `g_flOverrideNprOutlineThickness` (thick anime ink). `...Enemy` variant
  exists for asymmetric looks.
- **Outline removal:** add `F_DISABLE_NPR_OUTLINE=1`.
- **Unlit flip:** add `F_UNLIT=1` for full flat-shading (pairs with Tier B
  posterize for a real cel look).
- **NPR off:** zero `F_USE_NPR_LIGHTING` for an uncanny "realistic PBR hero".

### Tier B: texture-side cel look (existing texture pipeline)

- **Posterize albedo** (quantize V into N bands, keep H/S) via the existing
  decode -> edit -> `replace_mip_chain` path; combined with a flattened
  roughness B channel (Frostline/op-art recipe) and optionally `F_UNLIT`.
- **Rim mask boost:** `g_tTintMaskRimLightMask` is a flat 4x4 constant on
  shipped skins (G = rim constant). Overriding that tiny texture with a
  brighter constant (or a painted mask) is a texture-entry override, no vmat
  edit at all.
- **Outline mask:** paint `g_tNprOutlineMask` to control where ink appears.

### Tier C: cel-band attribute injection (speculative, one probe)

Materials have `m_floatAttributes` / `m_vectorAttributes` /
`m_renderAttributesUsed` tables (the int table is used for
`RepresentativeTextureWidth/Height`). If the engine resolves `__Attribute__`
shader vars against *material* attributes (it does for renderables; materials
are one of the attribute providers in Source 2), injecting
`g_flNPRDiffuseStepSharpness` / `g_nNPRSpecularSteps` per material would give
real cel-band control. No shipped material does this, so it needs an in-game
probe before designing around it. If it fails, banding is still reachable via
Tier B posterize.

## 3. Risks / unknowns

1. **Static-combo flips are unverified in-game.** Adding/zeroing an `F_` flag
   selects a different precompiled combo. All combos we need exist in the
   shipped `.vcs` and are used by other materials, but we have never flipped a
   flag on an *existing* material and confirmed it renders (the scroll-speed
   win patched floats only). This is probe #1.
2. **Attribute injection (Tier C) may be ignored** for these specific vars if
   the renderer binds them from a global constant buffer before material
   attributes are consulted. Cheap to test, no fallback cost.
3. **`m_renderAttributesUsed`** may need the attribute name appended as well
   (string array, `set_strings_adding` territory).
4. **Outline thickness has one shipped datapoint** (Infernus translucent), so
   extreme values are untested by Valve content; clamp in the CLI.

## 4. Probe plan (loose files + `mat_reloadallmaterials`, hero_testing map)

1. Vindicta dress, patch existing `g_vSolidOutlineTint` to white: proves value
   edits on outline params (lowest risk, floats only).
2. Same material, add `F_DISABLE_NPR_OUTLINE=1`: proves int-param *insertion*
   flips a static combo.
3. Add `F_OVERRIDE_NPR_OUTLINE=1` + thickness 4x: proves the ink look.
4. Add `F_UNLIT=1` on a posterized-albedo build: the full cel-shaded probe.
5. Tier C: inject `m_floatAttributes` `g_flNPRDiffuseStepSharpness=8` and eye
   the diffuse terminator. A/B with `r_citadel_disable_npr_lighting` /
   `r_citadel_npr_force_solid_outline` to learn the baseline.

## 5. Shipped: `vpkmerge vmat` (2026-06-10)

`vpkmerge_core::vmat_style` + the `vmat` CLI command implement Tier A as a
generic set-or-insert param patcher plus curated presets:

```
vpkmerge vmat --vpk <VPK> [--base <VPK>] (--hero CODENAME | --entry PATH...)
    [--list]
    [--preset gem|glass|pbr|unlit|ink] [--tint R,G,B|#RRGGBB]
    [--set-int NAME=V]... [--set-float NAME=V]... [--set-vec NAME=X,Y,Z[,W]]...
    [--targets all|body|weapons] [--encode-vpk OUT_dir.vpk]
```

- `--list` prints each targeted material's shader, nonzero `F_*` flags, and
  bound texture channels (the "what is this skin made of" view).
- Presets: `gem` (constant-only sheen, recipe = `xmas_vindicta_dress`),
  `glass` (recipe = `viscous_body` minus its mask texture), `pbr`
  (`F_USE_NPR_LIGHTING=0`, real reflections), `unlit`, `ink` (thick solid
  outline). `gem`/`ink` take `--tint`.
- Patch engine: byte-faithful in-place set (`patch_kv3_resource_scalars` /
  `_doubles`) or structural insert (`patch_kv3_resource_array_insert`);
  tagless 0/1 values fall back to a full `encode_kv3_resource` re-encode on
  non-blobbed materials, same discipline as `hero_recolor`'s tint stamping.
  Blobbed materials report the edit as failed instead.
- Output validated offline: VRF (`material-meta`) parses a gem-patched
  `vindicta_dress.vmat_c` with all 7 params landing, and `vpk_viewer` exports
  and renders the model from the probe VPK without breaking. Note the GLB
  pipeline does not translate sheen/glass/NPR params, so the viewer only
  proves the material is well-formed; the look itself is an in-game gate.

Probe addons (gem/pbr/glass dress + whole-hero ink, all Vindicta) are
installed and registered as Grimoire local mods, pak01..pak04, pending
in-game confirmation.

## 6. Artifacts

- `vpkmerge-core/examples/npr_vmat_survey.rs`: the survey tool (rerun after
  game updates; also useful to find heroes with unusual NPR setups).
- Full pbr param dump: regenerate with
  `morphic-oracle shader-dump --vpk citadel/shaders_pc_dir.vpk --entry shaders/vfx/pbr_pc_50_features.vcs`.
