# Grimoire Shader Preview Handoff

Date: 2026-06-19

## Goal

Continue Deadlock shader research with two tracks:

- Make Grimoire's 3D viewer closer to in-game material rendering.
- Find in-game shader experiments worth trying through vpkmerge.

Do not claim Viscous is fixed until the user visually validates it in the app.

## Current Grimoire State

Repo: `C:\Users\USER\grimoire`

Branch observed during this pass: `feat/npr-cel-shader`

Latest checkpoint:

- Grimoire commit: `492957e` (`Add unified Deadlock material preview path`)
- Fork backup branch: `oldreceipt/feat/npr-cel-shader`
- vpkmerge commit required for fresh GLBs: `75f8091`
  (`Add Source 2 preview metadata to GLB extras`)
- vpkmerge fork backup branch:
  `oldreceipt/feat/source2-preview-metadata-backup`

Important touched files:

- `src/components/locker/HeroPoseViewer.tsx`
- `src/lib/source2NprMaterial.ts`
- `src/lib/deadlockMaterial.ts`

Changes already made:

- `USE_RIGGED_PREVIEW` was set back to `false` in `HeroPoseViewer.tsx`.
  - Reason: Viscous 3D modal was blank with rigged preview enabled.
  - Static pose path renders Viscous again.
- `F_GLASS` handling in `source2NprMaterial.ts` no longer forces:
  - `mat.transparent = true`
  - `mat.depthWrite = false`
  - low `mat.opacity`
  - Reason: this made Viscous look like washed-out white glass instead of saturated green goo.
- `F_SELF_ILLUM` handling was tightened.
  - It should only affect emissive output when there is a meaningful self-illum map, explicit tint, or explicit scale.
  - Reason: many Viscous materials have `F_SELF_ILLUM` plus default placeholder masks; treating that as full emissive made the body milky white.
- `grimoire.preview.unifiedMaterial` now enables a single owned-clone material
  builder (`deadlockMaterial.ts`) instead of layering Source 2 hints and NPR CSM
  mutations on the GLTF base material.
  - Default remains off.
  - The old renderer path remains present.
  - The new path keeps translucent/goo materials on alpha blend, not physical
    transmission.
- `grimoire.preview.celV2` now enables the experimental direct-diffuse cel pass.
  - It quantizes `reflectedLight.directDiffuse` at `lights_fragment_end`.
  - Rim, self-illum, IBL, and final texture color survive better than the older
    final-luminance posterize.
  - Default remains off.
- `applySource2MaterialHints` can skip NPR materials when unified mode is on.
  This lets unified mode own NPR materials while the legacy hint pass still handles
  non-NPR Source 2 materials such as glass and translucent surfaces.

Verification already run after edits:

- `pnpm typecheck` passed.
- `pnpm build` passed with `GRIMOIRE_SOCIAL_BASE_URL` set to a temporary HTTPS
  value.
- Dash-rule search on touched Grimoire shader/viewer files was clean.

## Visual Validation Status

Validated failure states:

- With `USE_RIGGED_PREVIEW=true`, Viscous modal can be blank.
- With the earlier broad shader hints, Viscous rendered but looked wrong: pale, milky white, not like in-game Viscous.
- The likely bad assumptions were glass alpha fading and broad self-illum application.

Not yet validated:

- The latest unified/celV2 work still needs a structured hero-by-hero visual pass.
- Do not flip `USE_UNIFIED_MATERIAL`, `USE_CEL_V2`, or `USE_NPR_PREVIEW` on by
  default until Viscous plus at least two other shader-heavy heroes are checked.

Suggested validation flow:

1. Build Grimoire with:

   ```powershell
   $env:GRIMOIRE_SOCIAL_BASE_URL='https://example.com'
   pnpm build
   ```

2. Launch Electron using the current vpkmerge debug binary:

   ```powershell
   $env:GRIMOIRE_SOCIAL_BASE_URL='https://example.com'
   $env:VPKMERGE_BINARY='C:\Users\USER\vpkmerge\target\debug\vpkmerge.exe'
   .\node_modules\.bin\electron.cmd --remote-debugging-port=9334 dist\main\index.js
   ```

3. In app DevTools or console before opening the viewer:

   ```js
   localStorage.setItem('grimoire.preview.source2Shaders', '1')
   localStorage.setItem('grimoire.preview.unifiedMaterial', '1')
   localStorage.setItem('grimoire.preview.celV2', '1')
   localStorage.setItem('grimoire.preview.nprDebug', '1')
   localStorage.setItem('grimoire.preview.npr', '1')
   localStorage.removeItem('grimoire.preview.nprOutline')
   ```

4. Open Locker -> Viscous -> 3D.

Compare independently:

- old path: no `unifiedMaterial`, no `celV2`
- unified only
- unified plus `celV2`

The target is not "more transparent at all costs." The target is saturated green
translucent/glossy goo with darker internal mass and readable black gear.

Cache warning:

- Existing GLBs do not automatically gain the new `morphic.extras` fields.
- Cleaning Grimoire's local preview cache deletes `hero-poses`, so the next hero
  preview re-runs `vpkmerge model export --pose`.
- In dev, Grimoire uses `VPKMERGE_BINARY` first, then
  `..\vpkmerge\target\release\vpkmerge.exe`, then the bundled downloaded binary.
  Rebuild vpkmerge release before re-exporting:

  ```powershell
  cargo build --release --manifest-path ..\vpkmerge\Cargo.toml -p vpkmerge-cli
  ```

## Reference Direction

User feedback:

- Current wrong look: milky white translucent body.
- User suspects blending or related material behavior is involved.
- Need web/reference pass for actual Viscous before further tuning.

Likely next tuning areas:

- Treat `F_GLASS` as glossy/transmission/IOR only, not opacity.
- Treat `F_TRANSLUCENT`, `F_ADVANCED_TRANSLUCENCY`, and `F_ADDITIVE_BLEND` as the only paths that should alter alpha/depth write.
- Be careful with self-illum placeholder masks.
- Add a dev-only material stats log for Viscous so the user and next agent can see which materials were mutated.

## vpkmerge Shader Research Status

Repo: `C:\Users\USER\vpkmerge`

Branch observed during this pass: `fix/glb-pure-normal-discriminator`

Useful doc already updated earlier:

- `docs/spike-npr-toon-shading.md`

Top shader candidates beyond Geist:

- Viscous
- Nano / Calico shadow form
- Punkgoat border jitter materials
- Inferno arm/head glow
- Hornet / Vindicta glow and NPR materials
- Mirage cyclone/genie materials
- Dynamo void/glass
- McGinnis / Forge sheen and greenglass
- Bookworm / Paige lens and books

Use `vpkmerge model live-materials --all --summary` and per-hero `model live-materials --hero <codename>` as the source of truth for live draw-call materials, not generic material names.

Important warning:

- Do not over-focus on Geist. The user explicitly called out Viscous and other shader-heavy heroes.

## Phase Status After 2026-06-19

- Phase 0/1: implemented behind `grimoire.preview.unifiedMaterial`.
- Phase 2: dropped for goo. Transmission made Viscous worse; alpha blending is the
  correct preview fallback for translucent/goo materials.
- Phase 3: implemented in vpkmerge GLB extras and consumed by Grimoire with old-GLB
  fallbacks.
- Phase 4: not done. Do not delete the legacy material path yet.
- Phase 5: implemented behind `grimoire.preview.celV2`.
- Phase 6: still deferred until real RenderDoc/constants evidence.
