# Preview fidelity roadmap: matching Grimoire's three.js preview to in-game Source 2

Research notes (2026-06-11) on closing the gap between Grimoire's hero preview
(Electron + three.js, fed by morphic's `.vmdl_c` -> `.glb` export) and Deadlock's
actual in-game rendering. Sources: code audit of grimoire + morphic, VRF's
renderer shaders, and prior art from Dota 2 / CS2 web viewers.

## Why the preview looks "off" today

Deadlock heroes do not render as standard PBR. 502 of 605 hero materials set
`F_USE_NPR_LIGHTING=1` on `pbr.vfx` (see [spike-npr-toon-shading.md](spike-npr-toon-shading.md)):
cel-banded diffuse, rim light, toon outlines, light wrap. The in-game frame is
then lit by baked HDR cubemap probes and tonemapped with the Hable (Uncharted 2)
filmic curve.

Grimoire's preview today:

- Plain `GLTFLoader` into `MeshStandardMaterial`
  (`grimoire/src/lib/loadGltfPreview.ts:41-50`)
- One ambient light + two directional lights, no environment map, no PMREM,
  no tone mapping override (`grimoire/src/components/locker/HeroPoseViewer.tsx:195-199`,
  same pattern in `SoulContainerViewer.tsx:150-153`)
- Static posed GLB (skeleton baked out via `--pose`), textures embedded by
  morphic, outline/glow shells stripped at export

Result: a smooth gradient-shaded PBR render of a model the game draws cel-shaded,
with dead reflective surfaces and no rim light. The renderer setup, not the model
export, is the dominant divergence.

## Tier 1: renderer-only fixes (no Rust changes)

Highest impact per line of code. All live in grimoire.

### 1. IBL from the game's own skybox probes

`materials/skybox/` in pak01 ships 78 BC6H HDR cubemaps (1024px, CUBE_TEXTURE
flag), e.g. `sky_dl_dusk02_nosun_exr_*.vtex_c`, `dl_midtown_dusk_02_exr_*.vtex_c`.
These are the actual probes the engine lights heroes with.

- Decode one via morphic (recolor refuses HDR, but this is a decode-only path;
  if `morphic::decode` does not handle BC6H yet, that is the one decoder to add),
  export once to `.hdr`/`.exr`, ship it with grimoire.
- `PMREMGenerator.fromEquirectangular` (or fromCubemap), assign
  `scene.environment`.
- Keep a warm key directional + cool fill on top: Source 2 lights heroes with
  IBL + direct, not IBL alone.

Every metallic/glossy surface (weapons, buckles, eyes) currently renders dark
and flat because `MeshStandardMaterial` without an envMap has no environment
reflections at all.

### 2. Tone mapping

Source 2 / VRF post-processing uses the Hable (Uncharted 2) filmic tonemapper,
not ACES. Pipeline order in-engine: exposure -> bloom -> Hable -> linear-to-sRGB
-> LUT color correction.

- Quick approximation: `renderer.toneMapping = THREE.ACESFilmicToneMapping`,
  `toneMappingExposure ~= 0.8`.
- Exact: small post-pass with the Hable curve, constants
  `A=0.15 B=0.50 C=0.10 D=0.20 E=0.02 F=0.30 W=11.2`:

```glsl
vec3 hable(vec3 x) {
    float A = 0.15, B = 0.50, C = 0.10, D = 0.20, E = 0.02, F = 0.30;
    return ((x*(A*x+C*B)+D*E)/(x*(A*x+B)+D*F))-E/F;
}
vec3 tonemap(vec3 c, float exposure) {
    return hable(c * exposure) / hable(vec3(11.2));
}
```

- Always `renderer.outputColorSpace = THREE.SRGBColorSpace` (three.js default
  since r152, but set it explicitly).

### 3. Vertex colors for skin tones

Deadlock face/skin materials bind `g_tColor` to a 4x4 white placeholder and
carry the skin tone entirely in per-vertex `COLOR` (see
[findings-deadlock-skin-textures.md](findings-deadlock-skin-textures.md) section 6).
morphic exports `COLOR_0` when the material opts in
(`morphic/src/material/mod.rs:150-170` gates on `F_VERTEX_COLOR` etc.), but
three.js does not multiply vertex colors into base color automatically.

Post-load:

```js
gltf.scene.traverse((o) => {
    if (o.isMesh && o.geometry.getAttribute('color')) {
        o.material.vertexColors = true;
        o.material.needsUpdate = true;
    }
});
```

This is the likely cause of flat/white faces in current previews.

## Tier 2: morphic GLB exporter fixes

All in `morphic/src/model/glb.rs` unless noted.

### 4. Metalness is hardcoded to zero

`metal_rough_png` (`glb.rs:1059-1064`) always writes B=0. `g_tMetalness` /
`g_tMetalnessMask` are declared in the slot map (`material/mod.rs:116-123`) but
never decoded. Fix: decode the metalness mask into the ORM blue channel.
Channel packing reference (VRF `ShaderDataProvider.cs`, `vr_complex`/`complex`):

| Source 2 texture | channels |
|---|---|
| `g_tColor` | RGB albedo (sRGB), A = metalness or translucency mask |
| `g_tNormal` | RG hemi-oct normal XY, A = roughness |
| `g_tMetalness` | R = metalness |
| `g_tAmbientOcclusion` | R = AO |
| `g_tTintMask` | R = tint enable |

(Roughness from `g_tNormal.a` is already handled.)

### 5. Self-illum scale via KHR_materials_emissive_strength

`g_flSelfIllumScale1` can be far above 1 (Chrono's clock face: 3.649) but the
GLB hardcodes `emissiveFactor=[1,1,1]`. Emit `KHR_materials_emissive_strength`;
three.js r158+ reads it natively. Glow elements currently render at a fraction
of intended brightness.

### 6. Second UV channel

`glb.rs:635-649` emits only `texcoords[0]`; the vertex decoder collects all of
them (`mesh.rs:387`). Emit `TEXCOORD_1`+ so detail/AO passes that use a second
UV set stop silently losing it. (three.js `aoMap` wants UV2 anyway.)

### 7. Sheen / glass / unlit material variants

`F_SHEEN` (26 materials, e.g. `xmas_vindicta_dress`), `F_GLASS`
(`viscous_body`), `F_UNLIT` (13 materials) all flatten to plain
`pbrMetallicRoughness` today.

- `F_SHEEN` -> `KHR_materials_sheen`. Important: three.js
  `MeshPhysicalMaterial.sheen` uses the Charlie NDF + Neubelt visibility, which
  is exactly the cloth model in VRF's `pbr.slang`. This is a correct match,
  not an approximation.
- `F_GLASS` -> `KHR_materials_transmission` + `KHR_materials_ior`.
- `F_UNLIT` -> emissive-only (`KHR_materials_unlit` or zeroed lighting).
- Grimoire side: this requires `MeshPhysicalMaterial`; `GLTFLoader` upgrades
  automatically when these extensions are present.

## Tier 3: the NPR / toon look

The actual cel-shaded character of the game. Two halves: export the data,
shade with it.

### 8. Export NPR params and the tint/rim mask

- None of the NPR texture channels reach the GLB today:
  `g_tTintMaskRimLightMask` (R = per-pixel tint enable, G = rim light constant),
  `g_tNprOutlineMask`, `g_tNprTransmissiveColor`.
- None of the flags/params are emitted either: `F_USE_NPR_LIGHTING`, sheen
  tints, outline colors, `g_flNPRDirectLightWrap`, cel-band colors
  (`g_vNPROutlineBrightColor`/`DarkColor`), etc.
- Plan: emit shader flags + scalar/vector NPR params as glTF `extras` on the
  material, and embed `g_tTintMaskRimLightMask` as an extras-referenced texture.
  VRF's own glTF exporter uses `material.extras` for the same purpose.

Bonus: once the tint mask is in the GLB, the preview can show live
ability-color tinting, i.e. a faithful Prism/recolor preview, which is the
feature no other Deadlock mod manager can offer.

### 9. Toon shading in three.js

Inject via `onBeforeCompile` on the standard/physical material (or
THREE-CustomShaderMaterial) when `extras.F_USE_NPR_LIGHTING == 1`:

```glsl
// half-lambert diffuse ramp (Valve NPR lineage)
float hl = dot(N, L) * 0.5 + 0.5;
float diffuse = pow(hl, 2.0);
// or quantize into bands per g_flNPRDiffuseStepSharpness

// rim light, modulated by tint/rim mask G channel
float rim = pow(1.0 - abs(dot(N, V)), 2.0) * rimIntensity;
```

Note the in-game cel-band parameters are injected globally by the renderer
(no shipped material carries them as attributes), so band sharpness/colors will
need hand-tuning against in-game screenshots.

### 10. Outlines

In-engine outlines are inverted-hull shells (which morphic deliberately strips:
`glb.rs:990-992` drops `*_outline`, `*jitter*`, `*_glow`) plus a stencil-based
post-process. Practical three.js options, in order of effort:

1. Inverted-hull pass: re-render backfaces scaled along normals in flat color.
2. `EffectComposer` edge detection on depth/normals.
3. Stop stripping the shell meshes at export and flag them via extras so the
   viewer can render them with the right material (flat color, reversed cull).

## Smaller correctness items

- Normal maps: Source 2 uses hemi-octahedral RG encoding (bias constant
  `1.003922 = 256/255`, decode in VRF `utils.slang`). morphic already decodes
  to standard normals at export; if a raw-texture path is ever added to the
  viewer, the decode must come along.
- Color space on manually built textures: albedo sRGB, everything else linear.
  `GLTFLoader` handles this automatically; only matters for hand-wired textures.
- Morph targets: never exported (`targets: None`). No current hero needs them;
  latent gap only.

## Suggested order of attack

| Step | Change | Where | Impact |
|---|---|---|---|
| 1 | vertexColors=true traversal | grimoire viewer | fixes white faces |
| 2 | IBL from game skybox probe + PMREM | grimoire viewer (+ one-time BC6H decode) | reflective surfaces come alive |
| 3 | Tone mapping (ACES now, Hable later) | grimoire viewer | overall contrast/exposure match |
| 4 | Metalness mask into ORM | `glb.rs` | metal parts read as metal |
| 5 | KHR_materials_emissive_strength | `glb.rs` | glow elements at real brightness |
| 6 | Sheen/glass/unlit extensions | `glb.rs` + MeshPhysicalMaterial | cloth/glass/flat materials |
| 7 | TEXCOORD_1 export | `glb.rs` | detail/AO second UV |
| 8 | NPR params + tint mask as extras | `glb.rs` | unlocks 9 and live tint preview |
| 9 | Toon ramp + rim via onBeforeCompile | grimoire viewer | the actual Deadlock look |
| 10 | Outlines | grimoire viewer (+ optionally stop stripping shells) | final silhouette match |

Steps 1-3 are an afternoon and close most of the perceptual gap. Steps 8-9 are
the differentiator.

## References

- VRF renderer shaders (canonical Source 2 reimplementation):
  `Renderer/Shaders/` in https://github.com/ValveResourceFormat/ValveResourceFormat
  - `complex.frag.slang` (hero shader), `pbr.slang` (GGX + Charlie cloth),
    `environment.slang` (box-projected cubemap IBL), `utils.slang` (hemi-oct
    normal decode, color space), `post_processing.frag.slang` (Hable tonemap),
    `outline_post.frag.slang` (stencil outlines)
  - Channel packing: `ValveResourceFormat/IO/ShaderDataProvider.cs`
- Local groundwork: [spike-npr-toon-shading.md](spike-npr-toon-shading.md)
  (NPR param inventory + survey), [findings-deadlock-skin-textures.md](findings-deadlock-skin-textures.md)
  (texture slot table, vertex-color skin, calibration notes),
  `vpkmerge-core/src/vmat_style.rs` (NPR/sheen/glass preset params)
- Prior art (web viewers of Source/Source 2 models):
  - https://github.com/timkurvers/dota2-model-viewer (three.js; documents what
    glTF conversion drops: specular, rim, directional ambient)
  - https://github.com/pissang/dota2hero (ClayGL; full mask-channel hero shader)
  - https://github.com/SamJUK/CS3D (CS2 weapons; note `csgo_weapon` packs
    R=roughness G=metalness, opposite of glTF)
  - Shared lesson: convincing results came from real game cubemaps + rim light,
    not material tweaks.
- three.js: MeshPhysicalMaterial sheen/transmission/anisotropy,
  KHR_materials_emissive_strength (r158+), color management
  (https://www.donmccurdy.com/2020/06/17/color-management-in-threejs/),
  THREE-CustomShaderMaterial for clean lighting-chunk injection.
