# Hero Locker with live animation preview: feasibility

Investigation (2026-06-01) into a GUI "hero locker": pick a hero, select which
**parts** (props / body / weapons / abilities) get a trippy treatment, choose a
**style + animation**, and **live-preview the animation** before baking the addon
VPK. Grounded in the current GUI (`gui/`) and core (`vpkmerge-core/`).

## TL;DR

- **The locker UI + per-part selection + bake is easy:** it's a thin shell over
  the recipe structure and the existing `trippy-skin` / `trippy-vfx` builders. Days.
- **Live preview of the *animation* is feasible and much cheaper than it first
  looks**, because the trippy effect is *color + pattern over time*, not particle
  physics. The pattern generators are pure functions of `(u, v, phase)`; animating
  `phase` is a frame loop. No simulation, no WASM, no engine.
- **The honest fidelity ceiling is per tier** (below). A 2D animated swatch is the
  high-ROI MVP; a 3D mesh preview is the "wow" tier; a faithful in-browser particle
  sim is not worth it (the in-game install is ground truth).

## Scoping assumption

"Hero locker" = a view **in our GUI / the Grimoire desktop client**, not Deadlock's
in-game loadout screen (we can't inject UI into the running game). Preview happens
in our app; the bake still installs as a `pak0N_dir.vpk` addon (see
[[deadlock-addon-install-testing]]).

## What the parts map onto (no new model needed)

A hero's treatable surface is already fully described by the pinned **recipe**
(`hero_recolor::recipe_for`) plus the trippy target flags:

| Locker "part" | Recipe / option source |
|---|---|
| Abilities (particle FX) | `recipe.particle_prefixes` → the `.vpcf_c` set |
| Ability textures / props | `recipe.texture_entries` (e.g. the satchel/frog/deployable albedos we just pinned) |
| Tint materials | `recipe.material_entries` |
| Prop/ult mesh vertex color | `recipe.model_entries` (e.g. Paige's horse) |
| Body / weapon skin | `trippy-skin` discovery of `models/heroes*` + `include_body` / `include_weapons` |

So "select parts" = toggles over these buckets, fed straight into
`TrippyAbilityOptions { include_abilities, include_weapons, ... }` /
`TrippySkinOptions { include_body, include_weapons, ... }`. The builders already
exist (`trippy_skin_to_addon`, `trippy_ability_vfx_to_addon`).

**Gap today:** the frontend only sees recipe *counts* (`supported_hero_options` →
`HeroOption`, `lib.rs:261`). For a locker we expose the actual categorized entry
lists (one new command, below).

## Why animation preview is cheap here

The trippy animation has exactly two visual ingredients, both preview-able without
the engine:

1. **Pattern + UV scroll** (skins, ability textures). `trippy_pixel(style, u, v,
   phase)` and `paint_image(...)` in `trippy.rs` are pure and deterministic.
   Rendering the same tile at `phase = t` for `t = 0..1` *is* the scroll/flow
   animation. The in-engine `g_v*ScrollSpeed1` patch produces the same motion; our
   preview just advances phase per frame.
2. **Color over particle life** (the `loop` / `cycle` animation passes). This is a
   gradient swept across each particle's lifetime. Per [[blender-particle-translation]]
   and [[animated-prism-looped-gradients]], color-over-life needs **no simulation**:
   an animated gradient ramp / a representative tinted sprite conveys it faithfully.

Neither needs particle motion. What a faithful particle *sim* would add (sprite
counts, velocities, trails) is exactly what reads fine in-game and poorly in a tiny
preview anyway: low value.

## Tiered plan

### Tier 1: animated 2D swatch (recommended MVP)
**What:** an animated tile per selected style/part showing the pattern flowing +
the color cycling. Answers "what does *holo cycle* look like?"
**Backend (small):** one core fn, e.g.
`trippy_preview_frames(style, phase0, scroll, n) -> Vec<Vec<u8>>` (N PNGs): a loop
around the existing `paint_image`; mirrors `recolor_texture_preview_png`
(`recolor.rs:137`) and `recolor_hero_preview_png` (`hero_recolor.rs:1678`), which
are today single-frame. Expose as a Tauri command.
**Frontend (small):** play the frames in `<img>`/`<canvas>` on a
`requestAnimationFrame`/interval loop. (The GUI has no looping animation today:
`motion.css` is all <250ms transitions, so this is a new but tiny primitive.)
**Effort:** ~2-3 days. **Fidelity:** exact for pattern/scroll; representative for
particle color-cycle. **Risk:** low (reuses the bake's own generators → no drift).

*Alternative to the frame loop:* port `trippy_pixel` to a GLSL fragment shader and
animate a `phase` uniform in a `<canvas>` (smoother, zero per-frame IPC), but it
duplicates the pattern math (drift risk). Prefer Rust frames for the MVP; keep the
shader twin for Tier 2 where it's reused on the mesh.

### Tier 2: 3D mesh preview ("on the satchel / on the hero")
**What:** load the hero/prop GLB in a WebGL viewer and apply the animated trippy
material (UV scroll + pattern + Fresnel for holo). Answers "what does it look like
*on the prop*?"
**Backend:** new Tauri command over the existing `export_hero_model` /
`export_model` (`model.rs:102,122`) → GLB bytes (pose-frame aware, from the
pose-clip work). Already implemented core-side; just not wired to IPC.
**Frontend:** add `three.js` / `@google/model-viewer` (none today, confirmed no
3D/WebGL in `gui/package.json`); a `ShaderMaterial` reusing the GLSL twin from
Tier 1 for the animated look.
**Effort:** ~1-2 weeks. **Fidelity:** high for skins/props; still no live particle
FX. **Risk:** medium (new 3D dependency + shader parity with the bake).

### Tier 3: faithful particle playback / in-engine
**Verdict: not worth building.** A real particle sim in-browser is multi-week
(extract `.vpcf_c` operators to JS, or compile sim to WASM) for an effect that
already reads correctly the moment you install the addon. The real-fidelity path is
the existing in-game loop: loose files + `mat_reloadallmaterials` in the
`hero_testing` sandbox ([[deadlock-live-preview-netconport]]). The locker should
**link to / trigger that**, not re-implement the engine.

## Recommended MVP

A **"Locker" tab**:
1. Hero picker (reuse `supported_hero_options`).
2. Part toggles (abilities / body / weapons / props) + style picker
   (`TRIPPY_STYLE_NAMES`) + animation depth (off/sweep/loop/cycle) + intensity/scroll
   sliders.
3. **Tier 1 animated swatch** updating live as you change style/animation.
4. "Bake addon" button → `trippy_skin_to_addon` + `trippy_ability_vfx_to_addon`
   into the next free `pak0N` slot (reuse `default_addon_output_path`,
   `build_hero_prism_vpk` as the template, `lib.rs:323`).
5. Copy that says: the swatch is the *color/pattern* animation; full FX is confirmed
   in-game (one-click "open hero_testing" if we wire the sandbox).

New backend surface is small: `trippy_preview_frames` (Tier 1) + a categorized
`hero_recipe_parts(codename)` command. Everything else already exists.

## New core/IPC functions this would add

- `trippy::trippy_preview_frames(style, phase, scroll, n_frames, size) -> Vec<png>`
  (or APNG): Tier 1 preview generator.
- `hero_recolor::recipe_parts(codename) -> { abilities, textures, materials, models }`
  (exposes categorized entry lists to the frontend).
- Tauri: `trippy_preview`, `hero_recipe_parts`, `build_trippy_addon`,
  (Tier 2) `export_hero_glb`.

## Key file anchors

- Tauri IPC surface: `gui/src-tauri/src/lib.rs:127-585` (13 commands; none animated).
- Static texture preview path: `lib.rs:367` (`preview_texture`) → `App.vue:69-98` →
  `<img>` at `App.vue:822`.
- Prism tab (closest existing template): `gui/src/components/PrismTab.vue`.
- Hero list / counts: `lib.rs:261` (`supported_hero_options`).
- Pure pattern generators: `vpkmerge-core/src/trippy.rs` (`trippy_pixel`,
  `paint_image`, `holo`/`liquid`/… ).
- Single-frame preview precedents: `recolor.rs:137`, `hero_recolor.rs:1678`.
- GLB export (Tier 2, core-ready, not wired): `model.rs:102,122`.
- Bake builders: `trippy::trippy_skin_to_addon`, `trippy::trippy_ability_vfx_to_addon`.
