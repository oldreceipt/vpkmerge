# Deadlock NPR Preview Pipeline: Decision Handoff

## 1. VERDICT: Incremental-unify. Do NOT rewrite.

Collapse the three overlapping material systems into one build pass; do not green-field an NPR-first `ShaderMaterial`. The deciding reason: three of the four recurring regressions (milky-white body, see-through bones, transmission flip) are **shared-mutable-state and ambiguous-data-contract** bugs, not NPR-on-PBR bugs, so a rewrite would not fix them while it *would* forfeit MeshPhysicalMaterial's transmission render pass (three.js gates the entire pass on `material.transmission > 0` at three.module.js:7517/8232/8990/15450) and PMREM IBL that `viscous_body`/`head` (F_GLASS) specifically need. Reimplementing that on a raw `ShaderMaterial` is ~300 lines of `getIBLVolumeRefraction` + transmission render target + PMREM GLSL, and the TSL/NodeMaterial route is hard-blocked because `WebGLNodesHandler` does not support transmission in r184. The only correct "rewrite" instinct (cel-quantize before IBL combine) is a localized, independently-shippable GLSL edit (Phase 5), not a pipeline rewrite.

## 2. Target NPR pipeline architecture

**Single source of truth:** one new module `C:\Users\USER\grimoire\src\lib\deadlockMaterial.ts` exporting:

```
buildDeadlockMaterial(base, tuning, tintOverride)
  -> { material, uniforms, ownedTextures, dispose() }
```

Contract (one owner per property, never mutate the GLTF base):
- Reads `userData.morphic` **once**. The clone shares the `morphic` extras object **by reference**; treat it as immutable (do not write through `userData.morphic`; only write material *properties*).
- When physical features are needed and the base is `MeshStandardMaterial`, clone up: `const phys = new THREE.MeshPhysicalMaterial(); phys.copy(standard);` and work only on `phys`. The original GLTFLoader instance is never touched.
- Decides **all** material-state props once on `phys` (transparent, depthWrite, side, transmission, ior, opacity, blending, glass roughness/metalness, sheen), then builds the NPR CSM over `phys` so the CSM copies already-final state (kills the order dependency between today's two passes).
- Because the base is never mutated, the entire `before`-snapshot/`restore()` machinery (source2NprMaterial.ts:416-442, 563-590) and `unwrapNprBase` (817-834) are deleted. Teardown is just `dispose()`.
- **`dispose()` must dispose BOTH the CSM AND the underlying `phys` clone AND the owned mask clones.** CSM's inherited `dispose()` does NOT free its base material, so failing to dispose `phys` separately leaks one `MeshPhysicalMaterial` + its env program per hero load (a new leak the current code never had, since today there is no base clone).
- `tintOverride` drives `uTintColor` only (live recolor stays a uniform poke, no rebuild).

**CSM stays.** It is the cheapest way to keep IBL/skinning/shadows/transmission while injecting cel/rim. The bug-#3 landmine (CSM stamps `onBeforeCompile`/`__csm`/`customProgramCacheKey`/`uniforms` on the instance it is handed) is neutralized by handing it the throwaway `phys` clone, never the live GLTF base. CSM still mutates -- but it mutates the clone we own and dispose.

**Load-bearing inheritance to assert (the single most fragile link):** transmission renders through CSM only because CSM's `Object.assign(this, o)` copies `transmission` (and `isMeshPhysicalMaterial`) from `phys` onto the wrapper, and three reads `material.transmission > 0` on that wrapper object. This is undocumented CSM behavior. Guard it in dev: `console.assert(csm.transmission === phys.transmission)` plus a test. Do not treat "free transmission" as automatic.

**Per-pixel composition order (owned by the CSM in `buildDeadlockMaterial`):**
1. **Albedo + tint mask** (pre-light): `csm_DiffuseColor.rgb = mix(base, base*uTintColor, tintEnable)`. `g_vColorTint1` stays baked in `baseColorFactor`; do not re-apply. Kept verbatim.
2. **Cel diffuse + IBL** (v1): keep current post-`<opaque_fragment>` luminance posterize. Phase 5 replaces with quantization of the accumulated direct term.
3. **Rim**: `fresnel * lit-hemisphere gate(uWrap) * nprMask.g * uRimStrength * uRimColor`. Kept verbatim.
4. **Self-illum**: gated on `self_illum_valid`; scrolling `fract(uv + scroll*uTime)` mask * scale * tint, added after cel+rim. `emissiveIntensity` zeroed on the **owned clone** (one owner; restore stamp gone).
5. **Glass transmission**: one unconditional `transmission`/`ior`/`thickness` write on `phys` from `g_flIOR`. No `Math.max` against a second writer; bug #4 cannot recur.
6. **Advanced translucency**: `blend_mode`-driven `transparent` + `depthWrite=true` + `alphaMap`. The Standard->Physical upgrade makes `viscous_glass` (F_TRANSLUCENT, not F_GLASS) transmission/SSS writes land instead of silent no-ops. **Note: `blend_zwrite` (depthWrite=true on a transparent material) is a deliberate non-physical single-layer compromise; it occludes interior gear correctly but mis-sorts two translucent goo surfaces against each other. Acceptable for preview, not faithful.**
7. **Jitter**: keep displacement-map approximation for v1 (harmless on low-poly), flagged as approximation.
8. **Outline**: unchanged `buildOutlineShell`, inverted hull, gated independently.

**Data contract (kills bug #2 at the source):** in `morphic/src/model/glb.rs`, add to `morphic_extras` a `blend_mode` enum (`"opaque" | "blend_zwrite" | "blend" | "additive"`) derived from F_TRANSLUCENT/F_ADVANCED_TRANSLUCENCY/F_ADDITIVE_BLEND/F_GLASS at export, plus a `self_illum_valid: bool`. `buildDeadlockMaterial` reads `blend_mode` and sets full blend state from it -- but **falls back to flag-derivation when the extra is absent** (old cached GLBs), so the depthWrite heuristic is demoted to fallback, not deleted.

**Param flow:** `glb.rs morphic_extras` (data bus + `blend_mode` + `self_illum_valid`) -> GLTFLoader copies to `userData.morphic` -> `resolveMorphicTextures` fills `resolvedTextures` -> `buildDeadlockMaterial` reads once -> CSM uniforms. Engine-global `__Attribute__` constants live in one shared `NprTuning` exposed as live uniforms in the dev panel.

## 3. Phased migration plan

New flag `grimoire.preview.unifiedMaterial` (default off) runs the new path side-by-side with the old two-pass until validated. Each phase ships independently and is validated in the dev panel on Viscous.

- **Phase 0 -- Scaffold (no behavior change).** Create `src/lib/deadlockMaterial.ts`; `buildDeadlockMaterial` first cut just runs existing state logic + existing `wrapMaterialWithNpr` GLSL on an owned `.copy()` clone. Add `unifiedMaterial` flag in `HeroPoseViewer.tsx` (dev flags ~679-692). `NprMaterials` branches on it. Validate Viscous looks identical with flag on. **File targets:** new `deadlockMaterial.ts`, `HeroPoseViewer.tsx`.
- **Phase 1 -- Move material state into the build; delete snapshot/restore.** Port glass/translucent/sheen/backface/unlit/jitter decisions from `applySource2MaterialHints` into the build, on the owned clone. With flag on, `Source2MaterialHints` is a no-op. **Validate the three preserved fixes:** glass transmission present, translucent `depthWrite=true` (bones occluded), milky-white still gated (self-illum off without a real mask). `restore()`/`before` now unused on this path. **Files:** `deadlockMaterial.ts`, `source2NprMaterial.ts`.
- **Phase 2 -- Standard->Physical upgrade.** Upgrade `MeshStandardMaterial` to `MeshPhysicalMaterial` when **existing flags** (F_GLASS/F_TRANSLUCENT/F_ADVANCED_TRANSLUCENCY) require physical features, so SSS/transmission writes land. Gate on existing flags here (blend_mode does not exist yet). Validate Viscous outer goo (`viscous_glass`) reads translucent over the dark gear. **Files:** `deadlockMaterial.ts`.
- **Phase 3 -- Data-contract disambiguation (Rust + TS together).** Add `blend_mode` + `self_illum_valid` to `glb.rs morphic_extras`; swap Phase 2's flag gate and Phase 1's depthWrite gate to read `blend_mode`, **keeping flag-derivation as the fallback when the extra is absent** (old GLBs won't auto-re-export: `getHeroPoseInfo` returns `hasModel:true` for stale GLBs). Re-export preview GLBs (mtime bump cache-busts `meshUrlFor`). Validate goo vs additive both correct. **Files:** `morphic/src/model/glb.rs`, `deadlockMaterial.ts`.
- **Phase 4 -- Flip default, delete dead code.** `unifiedMaterial` default on. Delete `applySource2MaterialHints`, `wrapMaterialWithNpr`, `unwrapNprBase`, and the old `NprMaterials` two-pass branch. Validate the full Viscous 8-material table. **Files:** `source2NprMaterial.ts`, `HeroPoseViewer.tsx`.
- **Phase 5 -- Targeted cel-injection rewrite (the one real shader change).** Replace the luminance posterize. **Correct injection point: quantize the accumulated direct term `reflectedLight.directDiffuse` POST-loop (at `<lights_fragment_end>`), NOT NdotL at `<lights_fragment_begin>`** -- three sums NdotL across lights in `RE_Direct_Physical`, so there is no single NdotL to band at `lights_fragment_begin`. Leaves IBL unbanded. Gate behind `grimoire.preview.celV2`; validate visually against v1. **Files:** `deadlockMaterial.ts` (`NPR_PATCH_MAP`).
- **Phase 6 (deferred).** Vertex-shader F_JITTER_VERTICES, specular cel banding, bright/dark outline colors. All blocked on RenderDoc capture of engine-global `__Attribute__` constants (`g_vNPRLightWeights`, `g_flNPRDiffusePbrBlend`, `g_nNPRDiffuseSteps`, specular steps, rim color source). Do not guess in code before capture.

## 4. De-risking spike (run FIRST, before Phase 0, ~1 hour)

Validate the single load-bearing inheritance chain (clone -> CSM `Object.assign` -> renderer property reads at three:7517/8232/15450) that everything rides on and that fails *silently* (opaque body, hidden bones -- exactly the recurring bug). In the dev panel on Viscous:

1. Build a throwaway `MeshPhysicalMaterial`; `.copy()` the loaded `viscous_body` standard material into it.
2. Set `transmission = 0.9; ior = 1.5`.
3. Wrap **that clone** in a bare CSM (passthrough, no NPR GLSL).
4. Assign to the mesh and confirm in one frame: (a) body renders translucent with the gear visible, and (b) `renderer.info` shows the transmission render target allocated.

If translucency survives clone->CSM, the "free transmission + IBL + `isMeshPhysicalMaterial`" assumption holds and you proceed to Phase 0. If not, you spent an hour instead of discovering it in Phase 4 after deleting the old path.

## 5. Disposition of current code

| Symbol / file | Disposition |
|---|---|
| `applySource2MaterialHints` (source2NprMaterial.ts) | **DELETE** (Phase 4). State logic moves into `buildDeadlockMaterial`; snapshot/restore gone because the base is never mutated. |
| `wrapMaterialWithNpr` | **ABSORB** into `buildDeadlockMaterial` (GLSL kept verbatim in v1), then **DELETE** the standalone (Phase 4). |
| `unwrapNprBase` | **DELETE** (Phase 4) -- base is never mutated, nothing to reverse. |
| `before`-snapshot / `restore()` (lines 416-442, 563-590) | **DELETE** (Phase 1 makes it dead on the new path). |
| CSM dependency (`three-custom-shader-material`) | **KEEP.** Cheapest path to IBL/skinning/shadow/transmission + cel/rim injection. Neutralized by handing it the owned clone, never the live instance. |
| A3 self-illum scroll GLSL (`fract(uv + scroll*uTime)` mask*scale*tint) | **KEEP verbatim**, but gate on `self_illum_valid` and zero `emissiveIntensity` on the owned clone (one owner). |
| `resolveMorphicTextures` | **KEEP unchanged** -- must run while the parser is live; correct and necessary in any path. |
| `buildOutlineShell` | **KEEP unchanged** -- already independent and correct. |
| `isNprMaterial` / `isMeaningfulMask` / `summarizeNprScene` | **KEEP.** |
| `NPR_VERTEX` / `NPR_FRAGMENT` / `NPR_PATCH_MAP` cel+rim+tint GLSL | **KEEP verbatim** in v1; `NPR_PATCH_MAP` is the only thing Phase 5 rewrites (post-loop direct-term quantization). |
| depthWrite heuristic (line 490) | **DEMOTE to fallback** (Phase 3), do not delete -- old GLBs lack `blend_mode`. |
| `MeshPhysicalMaterial` as base | **KEEP.** The whole no-rewrite case rests on its built-in transmission + PMREM. |
| `Source2MaterialHints` / `NprMaterials` mounts (HeroPoseViewer.tsx ~528-646) | **REFACTOR** to call only `buildDeadlockMaterial` once `unifiedMaterial` defaults on (Phase 4). |
| `dispose()` cleanup (NprMaterials ~596-599) | **EXTEND** to dispose the owned `phys` clone in addition to the CSM + `ownedTextures` (new requirement; CSM does not free its base). |