# Findings: Deadlock hero skins, file paths, and texture mapping

Notes gathered while building the texture-reskin round-trip (recoloring Vindicta's
dress to a "snow galaxy"). Captures the non-obvious parts of how Deadlock lays out
hero models/materials/textures and what that means for recoloring and retexturing.

All paths are inside `pak01_dir.vpk` at
`<Steam>/steamapps/common/Deadlock/game/citadel/`.

## 1. Codename vs display name (the `--hero` gotcha)

A hero's compiled **model** (`.vmdl_c`) is named by an internal **dev codename**.
Its **materials/textures** (`.vmat_c` / `.vtex_c`) are named by the **display name**.
These often differ.

| Display name | Model codename | Body model path |
|---|---|---|
| Vindicta | `hornet` | `models/heroes_staging/hornet_v3/hornet.vmdl_c` |
| Mina | `vampirebat` | `models/heroes_wip/vampirebat/vampirebat.vmdl_c` |

`vpkmerge model export --hero <X>` matches `<X>.vmdl_c` under `models/heroes*`, so it
needs the **codename**: `--hero hornet` works, `--hero vindicta` fails (there is no
`vindicta.vmdl_c`; only her crow and a nail-FX model carry "vindicta" in a model name).

Heroes live under `models/heroes_staging/` (~227 model entries) and
`models/heroes_wip/` (~172). The plain `models/heroes/` tree is essentially empty.
WIP heroes are rougher; staging heroes are more finished (see zoning, below).

## 2. Finding paths inside a VPK

The VPK directory stores path components **split** (extension, directory, and
filename as separate strings), so `strings pak01_dir.vpk | grep models/.../foo.vtex_c`
never matches a full path. Two reliable ways to enumerate:

- `vpkmerge model <vpk>` lists every `.vmdl_c` entry (full paths).
- `valve_pak::VPK::file_paths()` yields all entry paths (used by the `list_paths`,
  `dump_tex`, `tex_stats`, `mat_dump` morphic examples).

Materials are referenced by the model **cross-directory**. The `hornet_v3` model
binds its dress material to `models/heroes_staging/vindicta/materials/vindicta_dress.vmat`
(the shared `vindicta/` materials dir), not anything under `hornet_v3/materials/`.
Always resolve the material path from the model/`.vmat`, do not assume it sits beside
the model.

## 3. A `.vmat_c`'s texture slots

`pbr.vfx` (the hero shader) binds these texture params (dump with the `mat_dump`
example). The ones morphic reads for glTF export are marked.

| Slot | Meaning | morphic PBR slot |
|---|---|---|
| `g_tColor` | albedo | base color |
| `g_tNormalRoughness` | packed normal (RGB) + roughness (A) | normal + metallic-roughness |
| `g_tAmbientOcclusion` | AO | occlusion |
| `g_tSelfIllumMask` | emissive mask | emissive |
| `g_tNprOutlineMask` | toon outline mask | (not read) |
| `g_tNprTransmissiveColor` | NPR transmission | (not read) |
| `g_tTintMaskRimLightMask` | tint-enable (R) + rim light (G) | (not read) |

## 4. Material zoning decides whether a recolor is clean

Region separation in Deadlock is done by **splitting a hero into separate materials**,
not by masks. How finely a hero is split decides whether a recolor bleeds.

- **Vindicta (`hornet`, staging) is finely split:** `vindicta_dress`, `vindicta_hair`,
  `vindicta_head` (+ `vindicta_headv2`), `vindicta_props`, `vindicta_gun`. Recoloring
  the dress touches only the dress.
- **Abrams (staging) is even finer:** `coat`, `upper_body`, `lower_body`, `head`,
  `gun`, `book` as separate materials.
- **Mina (`vampirebat`, WIP) is coarse:** her whole upper body (coat **and** blouse)
  is one material `mina_upper`; skirt **and** stockings is `mina_lower`. There is no
  way to recolor the coat without also hitting the white blouse, because they share one
  texture and there is no mask to separate them (see below).

Rule of thumb: pick a finely-split staging hero for clean reskins; WIP heroes need
in-texture color-keying or hand-painted masks.

## 5. Tint masks are NOT spatial region masks

`g_tTintMaskRimLightMask` sounds like a per-pixel "which parts are tintable" map. It
is **a flat 4x4 constant** even on shipped skins (checked Mina, and Abrams' xmas skin):
R=255 (tint enabled everywhere), G = a rim-light constant. So you cannot use it to
isolate coat-vs-blouse. The color albedo's **alpha channel is also empty** here
(`min 0, max 1, mean ~0`), so there is no hidden region mask there either.

Conclusion: the only thing distinguishing two zones inside one material is the
**painted albedo color itself**. Recolor methods have to key off that.

## 6. Skin renders white in the `.glb` (vertex-color shader path)

Skin materials (e.g. `vindicta_head.vmat`) set `F_VERTEX_COLOR=1` and
`g_bMaskVertexColorTint1=1`, and bind `g_tColor` to a flat **4x4 white placeholder**
(`vindicta_head_vmat_g_tcolor_*`). In-game the skin tone comes from **vertex colors**
multiplied through the shader, not from a normal albedo.

morphic faithfully bakes that flat 4x4 as the base color, so exposed skin (face, arms,
hands, legs) renders **white** in the exported `.glb`, and the body skin can look
**black** where a head-only texture is stretched over body UVs. morphic also does not
currently export vertex colors. The real artist skin texture (`vindicta_head_color`,
2048x2048, proper skin tones) exists in the sibling `vindicta/` dir but is **not** what
`g_tColor` points to.

This is a **preview-only** issue and does not affect a dress/clothing recolor (you
never touch skin). Candidate morphic improvement: when `g_tColor` resolves to a tiny
flat placeholder on a vertex-color material, fall back to the artist `*_color` texture.

## 7. Recolor methods: what preserves design vs destroys it

This was the central lesson. Given one material whose albedo holds multiple colored
zones (Vindicta's dress = red coat + navy skirt + brass bullet bandolier):

| Method | Keeps fabric shading? | Keeps distinct accents? | Verdict |
|---|---|---|---|
| Luminance gradient / duotone map | yes | **no** (collapses all hue to one ramp) | worst; flattened the two-tone + bullets |
| Single "Color" blend tint | yes | **no** (flattens all hue to one target) | accents lost |
| **Zone-aware HSV remap** | yes | **yes** | correct |

The zone-aware remap (what shipped for Vindicta):

1. Decode the original albedo, convert to HSV.
2. Mask by hue: red coat (hue ~0.83-1.0) vs navy skirt (hue ~0.52-0.80).
3. Recolor each zone independently (coat -> ice blue, skirt -> lifted toward white),
   keeping each pixel's value so shading/seams survive.
4. Leave **low-saturation** pixels (`s < ~0.12`: brass bullets, neutral trim, dark
   detail) untouched.
5. Composite graphics on top (snowflakes, starfield).

The "snow galaxy" look: FFT 1/f cloud noise mapped through a blue palette, then
**multiplied by the original albedo luminance** so the nebula drapes like fabric
instead of looking like a flat decal, plus a multi-layer white starfield (faint dust,
mid specks, bright stars, glints) and a few 6-arm snowflake crystals.

## 8. Blender preview != in-game

Blender (glTF) renders flat **PBR**. Deadlock renders heroes through an **NPR / toon**
shader (`F_USE_NPR_LIGHTING=1`), plus scene ambient light tints everything. The same
texture reads noticeably darker and more affected by environment color in-game than in
Blender. **Tune final colors against the game**, not Blender: push brighter, cooler,
and more saturated to compensate for the toon shading and warm ambient.

## 9. Texture round-trip + addon install

The dress color texture is **BC7, 2048x2048, 10 mips** at
`models/heroes_staging/vindicta/materials/vindicta_dress_color_png_a192a2cd.vtex_c`.

Round-trip (see `morphic/examples/skin_pack.rs`):

1. Read the original `.vtex_c` from `pak01_dir.vpk`.
2. Decode it once to recover its **alpha channel** (Source albedo alpha can be a mask;
   preserve it).
3. Load the edited PNG (must match the texture's dimensions), copy the original alpha
   back in.
4. `morphic::replace_mip_chain(orig_bytes, &new_image)` re-encodes mip 0 in the
   texture's **native format** (BC7 here) and rebuilds the full mip pyramid, splicing
   it into the original resource envelope. Output is byte-size-identical to the original
   for fixed-block formats.
5. Pack an addon VPK containing the new `.vtex_c` at the **same internal path** so it
   overrides pak01.

Install: drop the VPK in `game/citadel/addons/` named `pakNN_dir.vpk` (existing addons
use that convention; `gameinfo.gi` auto-loads `citadel/addons`). Restart the game.
Disable by moving it into `addons/.disabled/`.

## Dev tools used (uncommitted-history reference)

`morphic/examples/`: `dump_tex` (decode .vtex->PNG), `tex_stats` (channel stats),
`list_paths` (list VPK entries), `mat_dump` (dump .vmat texture params), `tex_fmt`
(format/size/mips), `skin_pack` (the round-trip). These are throwaway dev tools; the
intended product is a `vpkmerge texture edit` subcommand (see
[handoff-texture-edit-cli.md](./handoff-texture-edit-cli.md)).
