# VRF model renderer vs grimoire three.js preview: gap report

Source: ValveResourceFormat (`master`) versus `morphic` (this repo) and the grimoire
three.js preview. Headline question: VRF can render thin emissive overlays on Deadlock
heroes (inferno, hornet_v3) and our three.js preview cannot. This pins down exactly why,
then maps the full renderer gap.

## 1. Executive summary

VRF renders a hero by loading the `.vmdl_c`, building one `RenderMaterial` per draw call
through a single "complex" uber-shader, and dispatching every draw call into ordered
passes (opaque, static-overlay, water, translucent, outline) with reversed-Z depth and
per-material blend/depth state. Our preview is a different shape: `morphic` decodes the
model to glTF 2.0 ahead of time (LOD0 + default mesh-group only), and three.js renders
that glTF with PMREM IBL + ACES, optionally re-styling materials through
`buildDeadlockMaterial`/CSM behind dev flags that all default to **false**.

**The headline defect is not a shader, blend, or sort gap. It is geometry deletion at the
`morphic` exporter.** `is_glow_material` (a `*glow*` substring test at `glb.rs:1328`)
deletes exactly the additive glow draw calls VRF renders, before three.js ever sees them.
The renderer findings about missing `renderOrder`/`polygonOffset` are real but strictly
secondary: they cannot matter when there is no mesh.

Two corrections to the obvious-but-wrong version of the story (both verified by reading the
code, and both *strengthen* the root cause):

1. The additive metadata is **not** currently in the GLB for these overlays. The extras
   that would carry `blend_mode='additive'` are emitted only for *surviving* primitives, so
   the dropped glow materials produce no glTF material and no extras at all. The data lives
   in the source `.vmat_c`; it would be emitted once the geometry is un-dropped.
2. Simply un-dropping the geometry is **not** standalone-shippable. On the default ship
   path (all dev flags off) the scene renders untouched GLTFLoader materials with no
   override traverse, so a kept additive glow primitive renders as the original
   "opaque white hull" regression unless additive blending is honored unconditionally or
   the unified material builder is enabled. The morphic fix and the renderer fix are a
   package.

Beyond the overlay, the largest standing gaps are: dynamic expressions are never evaluated
in *either* pipeline (so `$ent_health`-style param animation is dead everywhere),
`double_sided` is never exported, render-state survives only as a `userData.morphic` extras
string rather than core glTF, and skinning is capped at 4 influences. Most of these are
quality refinements on an already-working preview; the headline fix is small but
load-bearing.

## 2. The thin-overlay gap (headline)

### What a "thin overlay" actually is

A second draw call using a translucent **additive self-illum** material drawn over the
body. Not a shader feature, decal, second-layer, or render-mode trick. Confirmed by
`vpkmerge model live-materials` on both heroes. The flag signature is consistent across
both:

```
F_TRANSLUCENT=1  F_ADDITIVE_BLEND=1  F_SELF_ILLUM=1  F_WRITE_DEPTH_BEFORE_ALPHA_BLENDING=1
(usually F_JITTER_VERTICES=1, often F_DISABLE_NPR_OUTLINE=1)   shader: pbr.vfx
```

Two concrete forms:

- **(a) inferno — shared-mesh re-draw.** `inferno_armglow.vmat` and `inferno_headglow.vmat`
  are extra draw calls over the **same** opaque body meshes (`inferno`, `inferno_flames`,
  `flame_hair`). The opaque base is `inferno_body.vmat` on mesh `inferno`. Glow color comes
  from a `g_tSelfIllumMask` (`inferno_glow_*`). The `inferno` mesh is drawn opaque, then
  overlaid with two additive passes on the same geometry.
- **(b) hornet_v3 — dedicated thin shell.** `vindicta_glow.vmat` on a separate low-poly
  mesh part named `ghost_glow` (3736 verts, the smallest draw call). A distinct translucent
  shell rather than a co-located re-draw.

`F_WRITE_DEPTH_BEFORE_ALPHA_BLENDING=1` on both is the Source 2 equivalent of an opaque
depth-prepass before the additive blend; it is what makes `depthTest=true` + `depthWrite=false`
the correct three.js configuration for the kept overlay (test against the body's depth, do
not occlude).

### VRF path (renders correctly)

1. Parse the glow `.vmat_c` (`Material.cs`); `pbr.vfx` is not name-mapped, so it routes to
   the `complex` uber-shader (`ShaderLoader.cs:414`, `:430`).
2. `LoadRenderState`: `F_ADDITIVE_BLEND==1 -> BlendMode.Additive`,
   `F_TRANSLUCENT==1 -> IsTranslucent` (`RenderMaterial.cs:223-296`).
3. At load, `AddDrawCall` routes by material flags: not `IsOverlay`,
   `IsTranslucent==true -> DrawCallsBlended` (`RenderableMesh.cs:336-347`).
4. Per frame, `CollectSceneDrawCalls` pushes `DrawCallsBlended` into
   `RenderPass.Translucent` with a back-to-front camera distance (`Scene.cs:807-844`).
5. `RenderTranslucentLayer` sets `DepthMask(false)+Enable(Blend)` and draws with the
   additive `BlendFunc` `SrcAlpha/One` (`Renderer.cs:362-373`; `RenderMaterial.SetRenderState`
   additive branch).
6. `complex.frag.slang`'s `selfillum` path (`:164`, `GetStandardSelfIllumination :196`)
   emits the emissive color from `g_tSelfIllumMask` scrolled over `g_flTime` (`:519-532`).

Net: an additive, depth-test-only, emissive second pass over the body. Nothing is filtered
out.

### Our path (drops the overlay)

`morphic` decode (`mod.rs:183`) keeps LOD0 x default mesh-group, assembles `MeshPart`s,
then `glb.rs build()` emits one glTF primitive per draw call. But `add_mesh` applies
`is_dropped` at **both** part-name and per-primitive-material granularity:

- `glb.rs:500` — `if is_dropped(&part.name) { return None }` -> drops hornet's `ghost_glow`
  **part** by name.
- `glb.rs:506` — `.filter(|p| !is_dropped(&p.material))` -> drops inferno's
  `inferno_armglow`/`inferno_headglow` **primitives** by material name off the shared body
  mesh (the body primitive survives; the glow primitive is deleted). This is per-primitive,
  not per-part: on a `MeshPart` whose primitives include both `inferno_body` and
  `inferno_armglow`, only the `armglow` primitive drops.

`is_dropped -> is_shell -> is_glow_material` (`glb.rs:1328-1337`, `:1360`) is a pure
substring test:

```rust
// glb.rs:1328-1331
is_glow_material = lc.contains("glow") && !lc.contains("noglow")
// glb.rs:1336-1337
is_shell = is_outline_material || is_glow_material
```

So `inferno_armglow.vmat`/`inferno_headglow.vmat` and the part name `ghost_glow` all match.
**The overlay geometry is never written to the GLB.** And because `is_dropped` short-circuits
*before* `material_for` (`glb.rs:570`) and `morphic_extras` (`glb.rs:1096`), the dropped
materials produce **no glTF material and no additive extras either** — the additive metadata
the renderer would key on is not in the file today. It is in the source `.vmat_c` and would
appear once the geometry is kept.

Downstream cannot recover it. The grimoire material builder *does* understand additive
(`blend_mode 'additive' -> THREE.AdditiveBlending + depthWrite=false`,
`deadlockMaterial.ts:328-331`), but there is no mesh to attach a material to. grimoire's
only geometry reconstructor is the inverted-hull outline shell (`buildOutlineShell`,
`source2NprMaterial.ts:1477`); there is no glow/overlay rebuild.

### Precise break point

`morphic/src/model/glb.rs:1328-1337` (`is_glow_material` / `is_shell`), reached via
`is_dropped` (`:1360`) and consumed at `glb.rs:500` and `glb.rs:506` inside `add_mesh`. The
`*glow*` substring match deletes the additive overlay before three.js is ever involved.
This drop was introduced intentionally (commit `aa96f71`, "drop glow shells") because an
additive glow shell collapses to an opaque white hull in plain glTF — but it is
**over-broad**, catching the real in-game overlays VRF renders.

> Note: `is_dropped` is name-based and runs on **both** the textured (`to_glb_textured`)
> and untextured (`to_glb`) export paths — there is no resolver-conditioned branch at the
> drop site. The fix must change `is_shell`/`is_glow_material` or remove the glow branch
> from `is_dropped`; it cannot be a "textured-path-only" carve-out.

### Fix sketch (the two halves are a package)

**morphic (load-bearing).** Stop treating glow as a droppable shell. Distinguish two cases
the current code conflates:
- *inverted-hull outline/jitter shells* (`is_outline_material`) — keep dropping; these are
  the white-halo geometry that motivated `aa96f71`.
- *additive self-illum overlays* (`F_ADDITIVE_BLEND` / `F_SELF_ILLUM`, or `*glow*` that is
  not an outline) — **keep**, and let `material_for`/`morphic_extras` emit the
  `blend_mode='additive'` + self-illum data so the renderer can composite. Ideally key the
  decision on the material flags (`F_ADDITIVE_BLEND`/`F_SELF_ILLUM`), not the `*glow*`
  substring, so it is robust to naming. Optionally also export `material.double_sided` and an
  explicit `is_overlay` extras flag.

**three.js + viewer.** Additive is already mapped (`deadlockMaterial.ts:328-331`). Add an
explicit `mesh.renderOrder` bump (e.g. 10) on additive/translucent overlay meshes so they
draw in the transparent pass after the opaque body; keep `depthTest=true`,
`depthWrite=false` (justified by `F_WRITE_DEPTH_BEFORE_ALPHA_BLENDING`). Ensure the
self-illum gate admits these (the overlay is `F_SELF_ILLUM=1`) so the CSM glow path runs.
No `polygonOffset` needed for the additive case — additive color is order-independent and
`depthWrite=false` handles inferno's coincident re-draw without z-fight.

**The dependency that makes this shippable:** on the default ship path all dev flags are off
(`HeroPoseViewer.tsx:79-124`), the scene renders untouched GLTFLoader materials with no
override traverse, and `isNprMaterial` requires `F_USE_NPR_LIGHTING===1`
(`source2NprMaterial.ts:418-422`). A kept glow primitive on that path renders as a plain
GLTFLoader material — opaque unless the glTF carries `alphaMode=BLEND` — i.e. the original
white-hull regression. So un-dropping geometry is only safe if **either** the unified
material builder is enabled for hero previews **or** additive materials are forced to
`AdditiveBlending` unconditionally (independent of the dev flags). This is a finding, not an
open question: items 1, 2 and 6 in the backlog ship together.

## 3. Feature-by-feature gap

| Capability | VRF | morphic export | grimoire three.js | Severity |
|---|---|---|---|---|
| **Additive glow overlay geometry** | translucent 2nd draw call (`RenderableMesh.cs:336-347`) | **DELETED** by `is_glow_material` (`glb.rs:1328`,`:500`,`:506`) | can do additive (`deadlockMaterial.ts:328`) but no geometry arrives | **CRITICAL** |
| Additive blend mode | `BlendMode.Additive`, `SrcAlpha/One` (`RenderMaterial.cs:223-296`) | would be extras `blend_mode='additive'` (`glb.rs:1123-1153`) — but skipped for dropped mats | `THREE.AdditiveBlending + depthWrite=false` (`:328-331`) | Medium (works once geometry present) |
| Translucent (alpha) blend | `BlendMode.Translucent`, SrcAlpha/OneMinusSrcAlpha | `AlphaMode::Blend -> glTF BLEND` + extras | `transparent`, opacity from `g_flOpacityScale1` fallback 0.62 (`:300-326`) | Low |
| Transparent draw ordering | back-to-front per-node distance (`Scene.cs:840`, `MeshBatchRenderer.cs:48`) | n/a (offline) | `mesh.renderOrder` **never set**; relies on three centroid sort (`HeroPoseViewer.tsx:659-675`) | Medium |
| Overlay/decal depth bias | `PolygonOffsetClamp(0,64,0.0005)` for overlay/depthbias (`RenderMaterial.cs:488-491`) | none | **no `polygonOffset` anywhere** | Medium (only bites non-additive coincident layers) |
| Static-overlay pass + author order | `StaticOverlay` pass sorted by `OverlayRenderOrder` (`Scene.cs:823`, `MeshBatchRenderer.cs:89`) | none | none | Low (heroes use additive 2nd-pass, not F_OVERLAY) |
| `depthWrite` control | per-material (`RenderMaterial.cs:458`) | n/a | additive->false, blend->false, **blend_zwrite->true** (`:309`) | Low/Medium |
| `depthTest` control | reversed-Z `DepthFunc.Greater` (`Renderer.cs:678`) | n/a | **never touched**, default true | Low |
| Double-sided / backfaces | `F_RENDER_BACKFACES` disables cull (`:494`) | **never set** (`double_sided` default false, `glb.rs:731`) | `DoubleSide` only from `F_RENDER_BACKFACES` extras (`:271,276`) | Medium |
| Self-illum / emissive | scroll over `g_flTime` (`complex.frag.slang:196,519`) | emissive tex + `g_vSelfIllumTint` + KHR emissive_strength (`glb.rs:951`) | CSM additive glow; baked emissive zeroed (`:333-471`) | Low |
| Dynamic expressions (`$ent_*`) | **decompiled but NOT evaluated** (`Material.cs:150`; only csgo_environment, `:365`) | extras strings only (`glb.rs:1107`) | dynamic-expr peak as static envelope | Medium (dead in both = fidelity ceiling) |
| Shader-selection fidelity | all heroes -> `complex` uber-shader; `GameVfx_*` branch (`ShaderLoader.cs:286,293`) | bakes params into PBR + extras | CSM patches over MeshStandard/Physical | Medium (approximation both sides) |
| NPR cel / rim / outline | `F_SOLID_COLOR_OUTLINE` toon edge; SSS stubbed (`complex.frag.slang:86-89`) | NPR masks in extras | CSM cel+rim + inverted-hull `buildOutlineShell` (opt-in) | Medium (off by default) |
| POS/NORMAL/TANGENT | full | exported w/ bounds (`glb.rs:529-558`) | as glTF | None |
| UV1 / multi-TEXCOORD | full | all streams exported (`glb.rs:641-658`) | as glTF | None |
| Vertex COLOR | full | gated on `material_uses_vertex_color()` (`glb.rs:539`) — correctly kept where `F_VERTEX_COLOR=1` (e.g. `vindicta_head`) | `enableVertexColors` where present | Low (gate is correct) |
| Skinning influences | full | **capped at 4**; 8-influence reduced to top-4 (`mesh.rs:1200-1211`) | as glTF | Low/Medium |
| LOD / mesh groups | screen-size LOD + 64-bit mask (`ModelSceneNode.cs:588-655`) | **LOD0 + default group only** (`mod.rs:194`) | as exported | Low (static preview) |
| Glass / transmission | TranslucentShaders, glass family | KHR transmission+ior | MeshPhysical transmission/clearcoat (`:278-298`) | Low |
| Lighting / environment | engine lights + reserved BRDF/cubemap | n/a | PMREM IBL from real skybox HDR + key/fill + ambient (`HeroPoseViewer.tsx:433-459`) | Low |
| Sheen / cloth | `F_CLOTH_SHADING` F0 (`texturing.slang:356`) | KHR sheen | only if source already MeshPhysical (`:206`) | Low |
| Texture format coverage | most VTexFormat (IA88/EAC/RGB32F unimpl, `MaterialLoader.cs:432-438`) | decode subset; ZSTD mesh bufs unsupported | KTX2/DRACO/meshopt decoders unregistered (`loadGltfPreview.ts`) | Low |

## 4. Other notable gaps (beyond the overlay)

- **Dynamic expressions are dead in both pipelines.** VRF decompiles
  `m_dynamicParams`/`m_dynamicTextureParams` to strings but never evaluates them at render
  time (`Material.cs:150-164`; only csgo_environment color matrices run, `:365`). morphic
  emits them as extras strings only. Heroes whose tint/self-illum/UV is driven by
  `$ent_health`/`$ent_age` look static in preview. Parity means *neither* animates — a
  fidelity ceiling, not a divergence.
- **`double_sided` is never exported.** `material.double_sided` stays `false` for every
  material (`glb.rs:731,1083-1094`); the viewer can only set `DoubleSide` from a
  `F_RENDER_BACKFACES` extras flag (`:271`). Single-sided thin overlays and backface meshes
  cull incorrectly on the plain glTF path.
- **Render-state lives only in extras.** Additive blend, z-write, render layer and sort key
  are not representable in core glTF; they survive solely as `userData.morphic.blend_mode`
  (`glb.rs:1107-1153`) and are ignored on the default ship path.
- **Skinning capped at 4 influences** (`mesh.rs:1200-1211`): subtle deformation error on
  high-influence joints under animation; invisible in a static pose.
- **`blend_zwrite` forces `depthWrite=true`** (`deadlockMaterial.ts:309`). A thin
  *alpha-translucent* (non-additive) overlay carrying it would occlude and z-fight the body
  (no `polygonOffset` to lift it). The inferno/hornet overlays are additive so they dodge
  this; a future alpha decal would hit it.
- **No `renderOrder`/`polygonOffset`/decal scaffolding** in the default path. Coplanar
  layers rely on three's unstable centroid sort; additive masks it, alpha translucency does
  not.
- **All restyle is dev-flag gated off by default** (`USE_NPR_PREVIEW`,
  `USE_UNIFIED_MATERIAL`, `USE_SOURCE2_SHADER_HINTS`, `HeroPoseViewer.tsx:79-124`). The
  shipped preview shows untouched GLTFLoader materials, so none of the Source2-aware
  blend/self-illum handling is active for end users today. This is the multiplier on every
  "works in the unified builder" claim above.

## 5. Prioritized remediation backlog

| # | Item | Layer | Effort | Unlocks |
|---|---|---|---|---|
| 1 | Keep additive self-illum overlays in the GLB; key the keep/drop on `F_ADDITIVE_BLEND`/`F_SELF_ILLUM` (not `*glow*`); keep dropping only inverted-hull `is_outline_material` shells. Changes `is_shell`/`is_dropped` itself (fires on both export paths) (`glb.rs:1328-1337`,`:500`,`:506`) | morphic | **S** | inferno arm/head glow + hornet `ghost_glow` reach the preview, with `blend_mode='additive'` extras now emitted |
| 2 | Honor additive unconditionally OR enable the unified builder for hero previews, so a kept glow primitive composites instead of rendering as a white hull on the default path | viewer + three.js | **M** | Makes item 1 actually visible (these ship together) |
| 3 | Set `mesh.renderOrder` (~10) on additive/translucent overlay meshes after material build; confirm self-illum gate admits them | three.js + viewer | **S** | Overlays draw after the opaque body; CSM self-illum glow runs |
| 4 | Export `material.double_sided` from `F_RENDER_BACKFACES` instead of always false (`glb.rs:731,1083`) | morphic | **S** | Thin single-sided overlays + backface meshes render correctly even on the plain glTF path |
| 5 | Explicit `is_overlay`/`additive` extras flag so the viewer identifies overlays deterministically | morphic + three.js | **S** | Robust detection independent of naming |
| 6 | `polygonOffset` + `depthWrite` handling for `blend_zwrite`/coincident **alpha**-translucent overlays | three.js | **M** | Non-additive thin decals composite without z-fight (future-proofing) |
| 7 | Evaluate simple dynamic expressions per-frame (tint/self-illum scale) | three.js | **L** | `$ent_*`-driven animation (parity ceiling; VRF doesn't do this either) |
| 8 | Raise skinning influence cap to 8 (second JOINTS_1/WEIGHTS_1) (`mesh.rs:1200-1211`) | morphic | **M** | Correct deformation on high-influence joints |

Minimum viable headline fix = **items 1 + 2 + 3 together** (un-drop, composite, order).
Items 1 or 2 alone are not shippable.

## 6. Open questions / verify in-engine

- **inferno's coincident shared-mesh re-draw:** confirm additive + `depthWrite=false` is
  z-fight-free in three.js. VRF uses reversed-Z (`DepthFunc.Greater`); three uses standard
  depth, so the depth break-even differs. Verify visually rather than assuming.
- **`F_JITTER_VERTICES` visual significance:** morphic approximates jitter as a
  displacement map (noted crude in `deadlockMaterial.ts`). If the in-game glow visibly
  jitters, the static preview will differ.
- **hornet_v3 staging assets:** per CLAUDE.md staging dirs can be stale, but
  `live-materials` follows `draw_call_targets`, so these are the live draw calls. Re-confirm
  resolved `.vmdl_c`/`.vmat` paths against the shipped build before relying on exact names.
- **Self-illum scroll over `g_flTime`:** VRF animates the emissive UV. Verify whether the
  CSM self-illum path reproduces scroll speed/direction or only the static mask.
- **`F_RENDER_BACKFACES` on the glow materials:** not called out in the live-materials flag
  dump. If absent, the overlays are single-sided and item 4 matters specifically for them.
