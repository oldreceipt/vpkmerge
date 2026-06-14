# Customization frontier: generated effects, undiscovered formats, and the hero studio

Research notes (2026-06-11) from a four-agent sweep over pak01 and the repo,
scoped to three themes: prism-style generated effects, weird/undiscovered swap
mechanisms, and deep single-hero customization. All findings are from decoding
real game files with the repo's own tooling; key claims were round-trip tested
where noted. Companion doc: [preview-fidelity-roadmap.md](preview-fidelity-roadmap.md).

## Headline finds

1. **Every "prism-but-beyond-color" effect is buildable with shipped machinery.**
   Real ability `.vpcf_c` files were decoded and an edit round-trip tested
   (Yamato `yamato_infinity_slash_tag_glow.vpcf_c`, `m_flConstantRadius`
   2.0 -> 4.0, byte-exact patch confirmed). All ten proposed effects below need
   only the existing `patch_kv3_resource_doubles/scalars/strings` paths that
   power prism. No new codec.
2. **One small morphic fix unlocks 17 untouched files.** The v4 KV3 reader
   errors on binary-blob values ("binary blobs require KV3 v5"); the v5 reader
   already handles them. Adding the blob case to the v4 path in
   `morphic/src/kv3/reader.rs` unlocks `postprocessing/gamestate/killed.vpost_c`
   (death screen), all four `caldera_lane_*.vpost_c` per-lane color grades,
   `main_menu.vpost_c`, `hero_select.vpost_c`, `shiv_possessed.vpost_c`, and
   `yamato_shadow_form_*.vpost_c`.
3. **The animation layer is unexplored, already decodable, and the engine
   accepts overrides (in-game confirmed).** `.vnmclip_c`, `.vnmgraph_c`,
   `.vnmskel_c`, and `.vcd_c` are all KV3 v5 resources;
   `morphic::decode_kv3_resource` succeeds on every one today. Zero published
   animation mods exist in the Deadlock community (GameBanana, modding wikis,
   forums checked), and the Viscous/Kelvin pose transplant (see ladder below)
   proved clip overrides load in-game. Wide-open territory.
4. **Unused content sits in the pak.** Yamato has an unshipped Christmas body
   skin (`heroes_staging/yamato_v2/materials/yamato_shape_xmas.vmat_c` + color
   texture) and three scrapped abilities' worth of VFX (nightmare, chaff,
   flash_bomb). Unused-content unlocks are a feature category of their own.

## Theme 1: generated effects (prism siblings)

### Particle anatomy as found in real files

A `.vpcf_c` wraps a `CParticleSystemDefinition`. Editable surface:

- Root scalars (direct doubles/ints): `m_flConstantRadius` (1..200 across
  surveyed files), `m_nMaxParticles` (1..200), `m_flConstantLifespan`,
  `m_flSimulationTimeScale` (1.0 everywhere, i.e. an unused global knob).
- Section arrays of class-keyed operator objects: `m_Emitters`,
  `m_Initializers`, `m_Operators`, `m_ForceGenerators`, `m_Renderers`,
  `m_PreEmissionOperators`, `m_Children` (recursive).
- Most numeric params live inside `CParticleFloatInput` sub-objects as
  `m_flLiteralValue` with `m_nType=PF_TYPE_LITERAL`: emitter rate
  (`C_OP_ContinuousEmitter/m_flEmitRate`, 100-200/s), burst count
  (`C_OP_InstantaneousEmitter/m_nParticlesToEmit`), per-particle birth values
  (`C_INIT_InitFloat` with `m_nOutputField` attribute ids: 2=radius,
  3=lifespan, 4=rotation, 7=alpha, 9=spin, 10=turbulence, 12=trail length).
- Physics: `C_OP_BasicMovement/m_Gravity` (vec3; z=-600 true gravity, 25 =
  rising embers on lash_ring), `m_fDrag`; `C_OP_VectorNoise`,
  `C_INIT_InitialVelocityNoise/vecOutputMin/Max`;
  `C_OP_AttractToControlPoint/m_flForceAmount`.
- Renderers: `C_OP_RenderRopes` (`m_flTextureVScrollRate` -50..-1000,
  `m_flTextureVWorldSize` 150-2000, `m_flOverbrightFactor` 2-4,
  `m_flSelfIllumAmount`), `C_OP_RenderTrails` (`m_flMaxLength` ~500),
  `C_OP_RenderSprites`/`C_OP_RenderModels` (`m_hTexture`/`m_hModel` string
  refs, patchable via `patch_kv3_resource_strings_adding`).

Params typed `PF_TYPE_CONTROL_POINT_COMPONENT` (game-driven, e.g. AoE ring
radius) cannot be overridden by literal patching; skip them silently, the
particle keeps working.

### Proposed presets, ranked by leverage vs risk

| Preset | Edits | Re-encode? | Risk |
|---|---|---|---|
| Slow-mo / fast-forward | `m_flSimulationTimeScale` (+ lifespan compensation) | No | Very low |
| Ghost / ethereal | InitFloat attr=7 alpha x0.3, self-illum x0.5 | No | Very low |
| Trail stretch | scroll rate, `m_flMaxLength`, tiling | No | Low |
| Giant / miniature | radius + spawn sphere + trail length multiplier | No | Low |
| Zero-G / reverse gravity | flip/zero `m_Gravity[2]`, `m_fGravityScale` | No | Low |
| Frozen / static | zero speeds + noise + gravity, sim scale ~0.01 | No | Low |
| Overcharge | emit rate + max particles + overbright xN (cap 3x, skip files with max>128: perf) | No | Medium (fps) |
| Turbulence / chaos | velocity-noise vecs x3, drag 0.3 | No | Low-med |
| Sprite swap | renderer `m_hTexture` redirect (respect the existing `is_tiling_particle_texture` allowlist) | No | Medium |
| Strobe / pulse | short fade in/out + short life + high rate; or insert an oscillate op via the existing array-insert path | Maybe | Low |

Shape of the CLI: `vpkmerge style-fx --hero <CODENAME> --vpk <VPK>
--preset ghost|giant|slowmo|zerog|overcharge|chaos|stretch --encode-vpk <OUT>`,
composing passes over the recipe's particle prefixes exactly like
`recolor-hero`. Note `m_nMaxParticles` is `Value::Int`: scalars patcher, not
doubles.

## Theme 2: weird / undiscovered formats

### Classification (probed against real pak01 files)

| File | KV3 | Decode today | Re-encode |
|---|---|---|---|
| `scripts/light_styles.vdata_c` | v4 | yes | yes |
| `scripts/music_data.vdata_c` | v4 | yes | yes |
| `soundstacks/*.vsndstck_c` + `mixgraph.vmix_c` | v4 | yes | yes (730 KB graph; test identity round-trip) |
| `scripts/tagged_sounds/<hero>_anim_sounds.vdata_c` (47) | v4 | yes | yes |
| `scripts/ping_wheel_messages.vdata_c` | v4 | yes | yes |
| `scripts/tarot/*.vdata_c` | v4 | yes | yes |
| `scenes/conversations/*.vcd_c` | v4 | yes | yes |
| `postprocessing/*.vpost_c` (v5 subset: basepostprocess, blinded, match_intro, drifter, doorman, post_game_portrait) | v5 | yes | yes (writer emits v4; verify engine accepts) |
| `postprocessing/*.vpost_c` (v4+blob subset: killed, caldera*, lanes, shiv, yamato, main_menu, hero_select) | v4+blob | **no - reader gap** | after fix |
| `pulse/*.vpulse_c` | v4 | yes | pointless: server-side bytecode |
| `scripts/precipitation.vdata_c` | v4 | yes | empty shell, no params; dead end |
| `.vnmgraph_c` / `.vnmclip_c` | v5 | yes | metadata yes; pose blob needs codec |

### The good stuff, per file

- **`light_styles.vdata_c`**: 14 named dimmer patterns as KV3 splines
  (`m_spline` points + tangent modes LINEAR/SPLINE/STEPPED). `fast_strobe` is
  a 10 Hz square wave; `candle_1..3` are ~2.2 s flicker loops;
  `slow_strobe` spikes to y=2.083. New styles can be added under any name.
  Disco map lighting is a spline edit. Caveat: this file defines patterns;
  what assigns a style to a given map light is map-side.
- **`music_data.vdata_c`**: full music state machine. States with BPM
  (`EMusicState_Hideout` 155, brawl draft 95), sync modes (Resume /
  RandomMarker), kill-streak stingers 01..10, `EStinger_HeroDeath`,
  respawn/rejuvinator stingers, chord voicings as MIDI note enums
  (`EMidiNotePitch_3Bb`), and an arpeggiator (`Music.Arp.Hideout.Bell`,
  range Bb1-G6, UpDown). Remap the death stinger to the respawn fanfare,
  retune the hideout to 60 BPM, or rewrite the chord chart: all field edits.
- **`mixgraph.vmix_c`**: 23 named submix buses (`announcer`, `hero_vo`,
  `music`, `opponent_bus`, `VOIP`, `hit-victim-bus`, ...) each with DSP
  processor chains (reverb presets, a 100 Hz highpass on main). Bus-level
  announcer mute, cavernous reverb on enemies, bass unlock: all here.
- **`.vpost_c` (decodable subset)**: numeric tonemap (`m_flExposureBias`:
  base 0.899, match_intro 0.730, post_game_portrait -0.730), bloom
  (`m_flBloomStrength` 0.5), vignette (strength -0.586, warm tint
  [0.356,0.287,0.266]), and a 32^3 RGBA color-correction LUT.
  `drifter_darkness_caster` maps everything to the red channel only; copying
  its LUT into `basepostprocess_deadlock.vpost_c` (already re-encodable) gives
  permanent horror-movie mode. After the v4-blob fix: custom death-screen
  grade, per-lane mood themes (4 independent lane LUTs).
- **`tagged_sounds/<hero>_anim_sounds.vdata_c`**: binds anim events
  (footstep, mantle, jumpland, melee_swing, reload) to soundevent *name
  strings*. Pointing every hero's footstep at any other soundevent is a
  string edit. 47 heroes covered.
- **`ping_wheel_messages.vdata_c`**: 54 entries; label/message localization
  tokens, SVG icon paths, concepts. Icons + text client-side; the minimap
  marker/concept is server-visible.
- **`tarot/*.vdata_c`**: rune costs (25-100), board layouts, stat thresholds.
  Possibly server-validated; treat as uncertain.
- **`pulse/*.vpulse_c`**: server-side map logic bytecode (EntFire hooks).
  Decodes fine, but client edits do nothing online. Dead end.

### The animation layer (zero prior art anywhere)

All four formats are KV3 v5 in a standard resource envelope; morphic decodes
them today. `m_compressedPoseData` inside clips is a quantized i16 bone-track
blob (VRF `AnimationClip.cs` documents the layout) and is the only part
needing a new codec.

Feasibility ladder:

1. **Viscous/Kelvin hero-select pose swap: zero code. IN-GAME CONFIRMED
   (2026-06-11).** They are the only two heroes sharing a skeleton (`viscous`
   references `kelvin_v2/maya/kelvin.vnmskel`). Kelvin's
   `ui_hero_select.vnmclip_c` packed at Viscous's entry path
   (`vpkmerge-core/examples/pose_transplant.rs`, installed as an addon pak)
   makes Viscous strike Kelvin's crouched pose on the hero-select screen.
   The engine honors `.vnmclip_c` overrides from addon VPKs: no signature
   check, no version rejection. The animation frontier is open.
2. **Idle/run clip redirect within one hero.** `.vnmgraph_c/m_resources` is an
   array of plain clip path strings (e.g. `abrams_loco_set_idle.vnmgraph_c`
   lists 10). Same-skeleton redirects are a `patch_kv3_resource_strings`
   call.
3. **Speed-shifted animations.** `m_flDuration` is a top-level float on the
   clip; halving it should play 2x. Needs in-game verification (engine may
   resample from frame count instead).
4. **`.vcd_c` conversation remixing.** Actors are named strings
   (`hero_astro`), speak events reference soundevent names with timing.
   String/float patches; engine-side validation unverified.
5. **Pose editing (T-pose everything, cross-hero poses).** Needs the
   quantized-pose codec (decode/encode i16 tracks against per-bone
   `m_trackCompressionSettings`, 128 objects/clip typical). The decode half is
   documented (VRF `AnimationClip.cs`); see the reality check below for why
   the codec alone is not "play any animation correctly." Cross-hero
   additionally needs bone retargeting; nearly all heroes have unique
   skeletons (98-117 bones).

### Reality check: why many clips are NOT standalone animations

Field evidence from the Yamato dumps explains why lots of clips look broken
when played in isolation (e.g. in Source 2 Viewer):

- **Additive clips.** `m_bIsAdditive` on the clip means it stores bone
  *deltas* meant to be layered over a base pose, not absolute poses. Played
  standalone, the deltas land on the bind pose and look like garbage.
- **Masked / partial clips.** Skeletons carry `m_maskDefinitions`, and the
  graph set includes `yamato_upperbodyreplace`, `yamato_ability_mask`,
  `hero_motionadditives`, `hero_motionset_masks`: many combat/ability clips
  only own a bone subset (arms for shooting) and are blended by the animgraph
  over a locomotion base layer. Without the graph's layer stack, half the
  skeleton freezes.
- **Cloth bones are not in clips at all.** Hair/capes/dangly bits are
  simulated at runtime (model-side `DSTF` block; identified but undecoded,
  see notes in `docs/vmdl-glb-exporter.md` on the `feat/model-blender-reshape`
  branch). Even a perfectly decoded full-body clip plays with rigid cloth.
  Preview dodge that skips the format entirely: tag the cloth bone chains in
  the skeleton and run a cheap spring/verlet sim in three.js; approximate but
  reads right.

Consequences: full-body clips (`ui_hero_select`, `hideout_stand_idle`,
locomotion, reload) are the viable targets; ability/combat layer clips mostly
are not, short of evaluating the animgraph layering. And since graphs
reference clips by path with control parameters fed from game code, there is
no slot for *new* animations: the only entry point is **replacing an existing
referenced clip on a matching skeleton**.

### The taunt path

Given replace-only, an in-game taunt = repoint a clip the player can trigger
at will and that is cosmetically safe to sacrifice:

- **Reload** is the best candidate: triggered on demand (R), every hero has
  dedicated clips (`hero_reload_default` graph), and the reload *timer* is
  game logic that should not read the animation's length. Replace the reload
  clip with a dance: press R to taunt. Client-side only (only you see it).
- Runner-ups: item-cast (`hero_itemcast`), and `ui_pose`/`ui_hero_select`
  for hero-select poses (zero gameplay coupling).
- The replacement clip must target that hero's skeleton, which loops back to
  the pose codec (or a Blender export/encode path) to author it. The
  Viscous/Kelvin shared skeleton is the one zero-authoring case.

**Experiment results (2026-06-11, all in-game):**

1. PASSED: Viscous/Kelvin `ui_hero_select` clip swap (plain path override).
   Viscous strikes Kelvin's pose on hero select.
2. PASSED (proto-taunt): Yamato's `reload_idle`/`reload_idle_quick`
   overridden with her `ui_hero_select` pose clip; the pose plays on reload
   in live play. Observed: the reload layer blends into both standing and
   moving locomotion, so the idle-reload override shows while moving too.
   Press-R-to-pose works (`vpkmerge-core/examples/anim_experiments.rs`).
3. PASSED (graph redirect): patching the clip path string at
   `m_resources[i]` in `abrams_loco_set_idle.vnmgraph_c`
   (`patch_kv3_resource_strings_adding`) made Abrams' out-of-combat stand
   idle play the crouch idle. Graphs follow their `m_resources` strings:
   the real taunt mechanism works.

**Hitbox finding (important):** hitboxes ride the bones, so whoever
simulates the animation positions the hitboxes. In locally simulated play
(hideout/sandbox, where the client is the server) the hideout bot with the
redirected idle took hits at the *visual* crouch; shots at the vanilla
standing position missed. Implications: (a) in online matches the server
animates vanilla, so anim mods on OTHER heroes desync your view from their
authoritative hitboxes: a self-nerf, and a reason grimoire should scope
animation mods to the player's own hero and non-gameplay surfaces
(hero select, hideout); (b) your own hero is safe: the server and other
players see vanilla you.

Round 2 results (`anim_experiments2.rs`, same day):

4. PASSED (out-of-list redirect): stand idle -> `sleep_idle`, a clip NOT in
   the graph's original `m_resources`, loaded and played. Any same-skeleton
   clip is reachable from any graph slot: the full taunt-system requirement.
   (As a hitbox probe it was a dud: Deadlock's sleep is a STANDING pose,
   the sleep-debuff animation. C2 staged with `knockdown_large_loop`,
   genuinely on the ground.)
5. PASSED (duration patch): `m_flDuration` x2 on the weapon run clips
   produced visible slow-mo, but only at high velocity (after jump +
   speed buildup). Reading: the locomotion system stride-matches (scales
   clip playback rate to ground speed), which masks the duration change at
   low speeds. Slow-mo is a one-field patch, modulated by velocity-driven
   rate scaling.
6. PASSED (hitbox probe): stand idle -> `knockdown_large_loop` put the
   hideout bot flat on the ground, and the hitboxes followed: only the
   on-ground body registered hits. Bonus observation: shooting him snapped
   him into the standing flinch anim, confirming flinch is a separate
   full-body interrupt layer.

Round 3 results (`anim_experiments3.rs`):

7. PASSED (anim-sound remap): `scripts/tagged_sounds/abrams_anim_sounds.vdata_c`
   footstep soundevent -> `Abrams.Melee.Swing` works in-game (every step
   whooshes). First live confirmation of the tagged_sounds vdata route; the
   file exposes 11 bindings (footstep, mantle, jumpland, melee swings,
   reload start/round/end, charge step) so footstep/reload sound packs are
   one string edit each.
8. NULL (cross-hero redirect, first attempt) -> led to a discovery:
   **Viscous ships only 3 compiled clips** (2 ability + ui_hero_select).
   His graphs reference `models/heroes_wip/viscous/clips/*` paths that DO
   NOT EXIST in pak01, yet he animates fully. The engine resolves clips
   through an indirection beyond the `m_resources` strings, most plausibly
   filename fallback against the shared skeleton's clip library (Kelvin's).
   Implication: round 3's redirect-to-Kelvin's-file was a null edit (the
   engine already resolves there), and `m_resources` strings are
   load-bearing when the target exists (proven on Abrams) but not the whole
   story.
9. INCONCLUSIVE (interrupt-layer override): flinch -> select pose looked
   too close to a vanilla flinch to judge.

Round 3.5 (`anim_experiments4.rs`): E2 and G2 both INCONCLUSIVE, same root
cause: the probe clips read as "generic standing" at the relevant
timescale. E2 (Viscous idle slot -> Kelvin's select pose) looked natural,
consistent with the vestigial-graph hypothesis; G2 (flinch -> sleep_idle)
just looked like standing briefly, since a ~0.3s flinch window cannot show
sleep droop. Lesson for probe design: only displacement (on the ground)
reads instantly. `anim_graph_dump.rs` added for spelunking graph resource
lists.

Round 4 results (`anim_experiments5.rs`):

10. NULL (missing-path injection): packing a clip at the
    referenced-but-missing `heroes_wip/viscous/clips/weapon_stand_idle`
    path changed nothing. Clip resolution is baked at compile time
    (RERL/manifest level); the engine does not re-check source-path strings
    for late-added files. Injection without graph edits is dead; graph
    redirects (proven on Abrams) remain the mechanism.
11. PASSED-with-a-twist (interrupt layer): flinch -> knockdown produced NO
    flinch at all, for shots and melee. The override loaded (vanilla flinch
    suppressed) but contributed nothing visible. Cause found by scanning
    clip headers (`anim_clip_scan.rs`): all three flinch clips are
    `m_bIsAdditive=true` delta overlays; a non-additive full-body clip is
    inert in an additive blend slot. **Design rule for any clip-swap UI:
    swaps must match the slot's additive type** (Abrams: 36 additive vs 202
    non-additive clips).

Round 4.5 staged: G4, like-for-like additive swap, flinch ->
`landing_impact_idle` (0.97s additive heavy-landing squash). Shooting him
should show the landing wobble instead of a flinch.

Round 4.5 result: G4 PASSED, with a correction to rounds G-G3: flinch is
the MELEE-hit reaction (all heroes), not the bullet-hit reaction, so the
earlier "shoot him" protocols were testing the wrong trigger. Melee hits
now play the landing squash. Interrupt layer fully confirmed: overridable,
additive-typed, like-for-like swaps only.

Round 5 results (`misc_experiments.rs`), first in-game tests of the
non-animation routes:

12. PASSED (vpost): `basepostprocess_deadlock.vpost_c` tonemap
    `m_flExposureBias` 0.899 -> -1.5 via byte-faithful double patch on a v5
    vpost visibly darkened the world. The engine accepts patched vpost
    files: post-processing customization is live (death-screen and lane
    LUTs still gated on the v4-blob reader fix).
13. PASSED (soundevents pitch): pitch 0.5 on all 10 Yamato Wpn events via
    `SoundEvents::set_event_field` + re-encode = deep slow gun in-game.
    Reconfirms the spike's earlier volume verification on a second field.
14. WORKED, but reclassified (unused-content unlock): the Yamato xmas
    material override is mechanically just a path override, same as any
    community skin mod. The value is not the mechanism but the catalog:
    systematically scanning the pak for unshipped `*_xmas` / `holiday*` /
    seasonal variants and offering them as one-click unlocks.
15. INCONCLUSIVE (music stinger): death-stinger remap not clearly audible
    on sandbox death; the death SFX heard may be a different soundevent
    layer, or the stinger does not fire in sandbox. Needs a cleaner trigger
    (e.g. remap a kill-streak stinger and farm a streak).
16. NULL (light styles): no styled lights in the hideout; the route remains
    untested in-game. Would need a map area with flicker/strobe lights.
    KV3 lesson kept: spline scalars are stored as double, float32, OR int
    per-value, so patching needs a per-value type fallback chain (174/330
    points reachable; the rest sit in typed arrays).

## Theme 3: the hero studio (Yamato blueprint)

Yamato = 3,794 pak01 entries (~2.9% of the pak). Full census by bucket:

| Bucket | Scale | Existing tooling | Missing |
|---|---|---|---|
| Model/materials/textures | mesh `heroes_staging/yamato_v2/yamato.vmdl_c`; body/head/sword vmats + vtex | `vmat`, `texture --hue`, `model recolor` | body/weapon textures not wired into the `recolor-hero` recipe |
| Animation | 182 clips + 30 animgraphs + skeleton (`heroes_wip/yamato/`) | none | see animation ladder above |
| VO | 1,648 vsnd (932 pings, 670 gameplay lines, 46 emotes); per-hero kill lines, item-use lines, convo lines | `soundevents --swap-vsnd / --set` per event | bulk apply (all 670 events), voice-pack abstraction |
| Voice wiring | `generated_vo_hero_yamato.vsndevts_c` + 2 `.vrr_c` talker files | soundevents layer | `.vrr_c` decoder (who/when a line triggers) |
| Ability + weapon SFX | 97 ability vsnd + 83 weapon vsnd + `soundevents/hero/yamato.vsndevts_c` | swap/set per event | "weapon sound skin" bulk abstraction |
| VFX | 303 particles (284 ability + weapon_fx + status_fx) + particle materials | `recolor-hero`, `prism`, trippy styles: full coverage | per-ability toggle, scale/rate sliders (theme 1 delivers these) |
| UI | card/critical/gloat/mm/sm/vertical/bg/gun vtex + 4 ability icons + name SVGs + CSS | `texture --hue`, `portrait` | recipe wiring; `.vcss_c`/`.vsvg_c` not decoded |
| Data | subtrees in monolithic `scripts/heroes.vdata_c` / `abilities.vdata_c`; `yamato_anim_sounds.vdata_c` | `morphic::kv3` raw | per-field stat editor (and gameplay vdata likely server-side online) |
| Conversations | 24 `.vcd_c` match-start convos | none | vcd edit layer |
| Post-process | `yamato_shadow_form_death/resurrect.vpost_c` (unique pair in the whole pak) | none until v4-blob fix | shadow-form color editor |

Structural gotchas for the editor UI:

- **Two model trees.** Yamato's mesh lives in `heroes_staging/yamato_v2/`
  while skeleton + clips live in `heroes_wip/yamato/`; a studio must scan
  both. (Yamato is the only hero with no `.vmdl_c` under `heroes_wip/`.)
- **Buckets are not uniform across heroes.** Haze spot-check: she has 16+
  custom footstep vsnd and 27 ambient head particles; Yamato has neither
  (shared footstep pool, no `particles/heroes/yamato/`). The UI must be
  capability-driven per hero, not a fixed form.
- **Unused content**: `yamato_shape_xmas.vmat_c` (unshipped seasonal skin),
  nightmare/chaff/flash_bomb VFX (scrapped abilities), legacy `shadow_form/`
  SFX folder alongside the current `a4_shadow_form/`. Haze similarly has a
  `holiday2024` bells accessory. An "unlock hidden content" feature falls out
  of the census almost for free.

## Client-side vs server-validated

| Area | Assessment |
|---|---|
| Particles, post-process, light styles | client-side rendering |
| Music states, soundstacks, mixgraph, VO/SFX | client-side audio |
| Footstep tagged_sounds | what you hear is client-side; enemy-audio authority uncertain |
| Ping wheel | icon/text client-side; marker/concept server-visible |
| Tarot thresholds/costs | uncertain, possibly server-validated |
| heroes/abilities vdata stats | reportedly inert in online matches |
| Pulse scripts | server-side bytecode; client edits do nothing |

## Suggested next three builds

1. **v4 binary-blob support in the KV3 reader** (`morphic/src/kv3/reader.rs`):
   smallest change, unlocks 17 vpost files including the death screen and
   per-lane moods.
2. **`vpkmerge style-fx` presets**, starting with slow-mo + ghost + giant:
   pure reuse of the prism patch machinery over the existing hero recipes.
3. **Viscous/Kelvin pose swap** as the zero-code proof-of-concept for the
   animation frontier, followed by animgraph clip redirects
   (`patch_kv3_resource_strings`), with the quantized-pose codec as the next
   real morphic milestone.
