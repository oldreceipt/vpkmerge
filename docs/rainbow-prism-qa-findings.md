# Rainbow Prism QA Findings

Date: 2026-06-01

Inputs:

- Base VPK: `/home/esoc/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`
- Normal addons folder: `/home/esoc/.local/share/Steam/steamapps/common/Deadlock/game/citadel/addons`
- Main implementation: `vpkmerge-core/src/hero_recolor.rs`
- Local inspection tools used: `recolor_assets`, `modelrefs`, `vpcf_outline`,
  `readcolor`, `vmatdump`, `rainbow-scan`, and `vpkmerge split --verbose`

## General Failure Pattern

The first roster pass assumed most ability VFX could be recolored by patching
particle color fields under:

- `particles/abilities/<codename>/`
- `particles/weapon_fx/<codename>/`

That was not enough for the QA failures. Several abilities draw color from other
carriers:

- Hero-specific `.vtex_c` color/self-illum maps referenced by materials.
- `.vmat_c` tint constants such as `g_vColorTint*` and `g_vSelfIllumTint*`.
- Model-spawned ability props whose materials are not obvious from the particle
  texture scan.
- Extra particle roots outside the standard two roots.

Projected ground decals are the most fragile case. Many use a mostly white or
single-channel mask plus particle tint. If the prism pass boosts that tint or
turns the texture into one flat hue, the authored falloff disappears and the
effect reads as a filled disk/square/flat circle on the ground. The safer fixes
are:

- Paint rainbow bands over the texture's existing luminance/alpha for true
  hero-specific projected textures.
- Leave the projected decal particle vanilla when the material uses shared
  masks and only the surrounding beam/spark/core can be safely recolored.
- Force a high-contrast color on a ring particle when the material is shared and
  no safe texture carrier exists.

## Grey Talon / Archer

User report:

- `pak02 archer`: Grey Talon ultimate did not change color.

What was wrong:

- The guided-arrow ultimate uses more than particle color fields. The initial
  recipe did not include the projectile/model color maps and explosion sphere
  carrier.
- `recolor_assets` only found the charged-shot gradient from the standard
  particle material walk:
  `materials/particle/abilities/archer/archer_charged_shot_gradient_color_v2_psd_51e62704.vtex_c`
- `modelrefs` showed the ultimate references model assets:
  `models/heroes_staging/archer/archer_guided_arrow.vmdl`,
  `models/heroes_staging/grey_talon/grey_talon_owl.vmdl`, and
  `models/heroes_staging/archer/archer_arrow.vmdl`.

Paths added to the recipe:

- `materials/particle/abilities/archer/archer_charged_shot_gradient_color_v2_psd_51e62704.vtex_c`
- `materials/particle/abilities/archer/archer_charged_shot_gradient_color_psd_17c02a47.vtex_c`
- `materials/particle/abilities/archer_guided_arrow_explosion_sphere_vmat_g_tcolor_a84e2808.vtex_c`
- `materials/models/particle/archer/archer_arrow_illum_vmat_g_tcolor_7d46cca1.vtex_c`
- `models/heroes_staging/archer/materials/archer_guided_arrow_color_psd_edd3d0f5.vtex_c`
- `models/heroes_staging/archer/bird/materials/bird_color_psd_117d09e0.vtex_c`

Still-risky paths if the owl/ult remains off:

- `models/heroes_staging/grey_talon/materials/grey_talon_owl_body.vmat_c`
- `models/heroes_staging/grey_talon/materials/grey_talon_owl_eyes.vmat_c`
- `models/heroes_staging/grey_talon/materials/grey_talon_owl_body_color_png_7ea4c6da.vtex_c`
- `models/heroes_staging/grey_talon/materials/grey_talon_owl_body_color_png_174cf8c6.vtex_c`

Reason for caution:

- These look like the actual owl body/eye materials. Recoloring them could tint
  the full owl model rather than just the ability glow, so they need in-game
  confirmation before adding.

## Bebop

User report:

- `pak03 bebop`: Hyper Beam ultimate left square/round filled texture artifacts.

What was wrong:

- Bebop had no safe hero-specific chromatic texture targets in the standard scan.
  The issue came from particle tinting on projected ground decals.
- `particles/abilities/bebop/bebop_laser_beam_proj_ground.vpcf_c` draws:
  `materials/particle/projected/bebop_laser_aoe_projected.vmat_c`
- That material uses a mostly white color mask:
  `materials/particle/abilities/bebop/bebop_laser_beam_ground_projected_psd_fe31601b.vtex_c`
- The prism pass turned the decal tint into a bright filled disk. Earlier
  brightness guards reduced this, but the artifact stayed visible.

Final fix:

- Leave these projected decal particles vanilla:
  - `particles/abilities/bebop/bebop_laser_beam_proj_ground.vpcf_c`
  - `particles/abilities/bebop/bebop_laser_beam_end_proj_ground.vpcf_c`
  - `particles/abilities/bebop/bebop_laser_beam_end_proj_surface.vpcf_c`
- Keep the beam, sparks, source/target, perimeter, and other Hyper Beam
  particles rainbow.

Tradeoff:

- The problematic ground/surface decals no longer rainbow-shift, but the visible
  beam body still does. This avoids the filled decal artifact without touching
  shared projected materials.

## Mo & Krill / Digger

User report:

- `pak04 digger`: burrow ground circle was too bright.

What was wrong:

- Burrow uses hero-specific dark projected ground self-illum textures. The normal
  prism texture recolor and particle value boost made the ground read too hot.

Relevant texture paths:

- `materials/particle/abilities/digger/digger_burrow_channel_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/abilities/digger/digger_burrow_explode_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/abilities/digger/digger_burrow_spin_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c`

Fix:

- Animated prism now uses projected-texture rainbow for Digger burrow textures,
  preserving the source mask and scaling value down to `0.68`.

Remaining risk:

- If it still feels too bright, lower only the Digger projected profile value
  scale again. The recipe coverage itself looks complete.

## Dynamo

User report:

- `pak07 dynamo`: could not see ult border; stomp missing edges; healing ability
  missing edges.

What was wrong:

- The initial recipe missed non-standard particle roots and model/material tint
  carriers.
- Dynamo uses extra roots:
  - `particles/dynamo/`
  - `particles/status_fx/status_fx_dynamo`
- The standard texture scan only reported one hero-specific projected texture:
  `materials/particle/projected/dynamo_void_sphere_projected_ground_vmat_g_tselfillum_670d93d.vtex_c`
- Several visible borders are actually sprite/rope particle gradients using
  shared textures, not safe hero-specific texture entries.

Paths added to the recipe:

- Particle roots:
  - `particles/abilities/dynamo/`
  - `particles/weapon_fx/dynamo/`
  - `particles/dynamo/`
  - `particles/status_fx/status_fx_dynamo`
- Textures:
  - `materials/particle/projected/dynamo_void_sphere_projected_ground_vmat_g_tselfillum_670d93d.vtex_c`
  - `materials/particle/abilities/dynamo/dynamo_void_sphere_marker_symbol_psd_73d6401e.vtex_c`
  - `materials/particle/abilities/dynamo/dynamo_void_sphere_planet_symbol.vtex_c`
  - `materials/particle/abilities/dynamo/dynamo_void_sphere_sun_halo_symbol.vtex_c`
  - `materials/particle/abilities/dynamo/dynamo_void_sphere_sun_symbol.vtex_c`
- Materials:
  - `materials/models/particle/dynamo_void_sphere_cyl.vmat_c`
  - `materials/models/particle/dynamo_heal_buff_model.vmat_c`

Important reviewed particles:

- Ult/black-hole edge:
  - `particles/abilities/dynamo/dynamo_singularity_rings_edge.vpcf_c`
  - `particles/abilities/dynamo/dynamo_singularity_rings.vpcf_c`
  - `particles/abilities/dynamo/dynamo_void_sphere_end_ring.vpcf_c`
- Stomp/gravity wave:
  - `particles/abilities/dynamo/dynamo_gravity_wave_rings.vpcf_c`
  - `particles/abilities/dynamo/dynamo_gravity_wave_beam.vpcf_c`
  - `particles/abilities/dynamo/dynamo_gravity_wave_trail.vpcf_c`
- Healing edge:
  - `particles/abilities/dynamo/dynamo_heal_buff_aoe_preview_band.vpcf_c`
  - `particles/abilities/dynamo/dynamo_heal_buff_aoe_preview_proj.vpcf_c`
  - `particles/abilities/dynamo/dynamo_heal_buff_proj_ground.vpcf_c`

Why edges may still need work:

- Some edge particles use shared sprite/rope textures such as `particle_ring_wave`,
  `astrological_ring`, `beam_generic_2`, and `beam_edge_05b`.
- Their colors are particle gradients, so they are recolored, but not backed by
  hero-specific texture maps.
- If the border is still hard to read, the next fix should be per-particle
  high-contrast color/value overrides for these ring/band particles, similar to
  the Haze ult ring special case.

Reviewed but not safe as texture additions:

- `materials/particle/abilities/dynamo/dynamo_singularity_ground_projected.vmat_c`
  uses shared `materials/particle/particle_modulate_01_color_tga_86adda0e.vtex`.
- `materials/particle/abilities/dynamo_heal_buff_ground_projected.vmat_c` uses
  shared `materials/particle/projected/circular_generic_projected_decal_psd_48e0061d.vtex`.

## Victor / Frank

User report:

- `pak09 frank`: 3D ability spawned a unicolor circle on the ground.

What was wrong:

- Victor's ground/projected effects include hero-specific projected self-illum
  maps. Single-hue texture recolor made them read flat.
- `frank_painaura_sphere.vmat_c` was added as a material candidate, but the
  prism generation reported no patchable material tint for it.

Texture paths in recipe:

- `materials/particle/abilities/frank/frank_painaura_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/abilities/frank/frank_revive_marker_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/frank_shock_miss_projected_bright_vmat_g_tselfillum_670d93d.vtex_c`

Fix:

- Animated prism now uses projected-texture rainbow for the three Frank projected
  textures, preserving luminance/falloff and scaling value to `0.88`.

Known limitation:

- `materials/particle/abilities/frank/frank_painaura_sphere.vmat_c` did not
  expose a patchable static prism tint in the current material patcher.

Reviewed extra ground paths:

- `materials/particle/abilities/frank/frank_revive_death_ground_projected.vmat_c`
- `materials/particle/abilities/frank/frank_revive_death_ground_dark_projected.vmat_c`
- `materials/particle/projected/frank_shock_projectile_ground_projected.vmat_c`

Reason for caution:

- These use shared ground/crater/burst texture maps or non-color normal maps.
  Recoloring their shared texture dependencies would bleed outside Victor. If
  one of these remains visually wrong, handle it by particle special-case or
  material-specific tint logic, not by adding the shared texture.

## Haze

User report:

- `pak10 haze`: could not see the circle border of ultimate.

What was wrong:

- The ult ring is:
  `particles/abilities/haze/haze_flurry_ground_proj_ring.vpcf_c`
- It draws shared material:
  `aoe_circular_falloff_projected.vmat`
- There is no safe hero-specific ring texture to add. The particle has one
  constant color field; the generic prism hash landed on a low-visibility color.

Fix:

- Special-cased `haze_flurry_ground_proj_ring` to force a bright gold/yellow
  color for visibility:
  - hue `52 + hue_offset`
  - full configured saturation/value

Tradeoff:

- The ring is high-visibility rather than fully rainbow. This is intentional
  because the underlying projected material is shared.

Potential follow-up:

- If the cast telegraph also needs stronger edges, review:
  - `particles/abilities/haze/haze_bullet_flurry_cast_aoe_telegraph.vpcf_c`
  - `particles/abilities/haze/haze_bullet_flurry_cast_aoe_telegraph_band.vpcf_c`
  - `particles/abilities/haze/haze_bullet_flurry_cast_rings.vpcf_c`

## Viscous

User report:

- `pak19 viscous`: Puddle Punch was off; Goo Ball ultimate did not change color.

What was wrong:

- The first recipe did not cover enough model/material carriers. Viscous uses
  ability model materials for fist/cube/slime/ball, not just particle color.
- Viscous also uses an extra melee particle root:
  `particles/abilities/melee/viscous/`

Recipe additions:

- Particle roots:
  - `particles/abilities/viscous/`
  - `particles/weapon_fx/viscous/`
  - `particles/abilities/melee/viscous/`
- Key textures:
  - `materials/models/particle/viscous_puddle_telegraph_vmat_g_tcolor_ac749641.vtex_c`
  - `models/heroes_staging/viscous/materials/viscous_punch_preview_vmat_g_tcolor_32414205.vtex_c`
  - `models/heroes_staging/viscous/materials/viscous_punch_vmat_g_tcolor_afc99362.vtex_c`
  - `models/heroes_staging/viscous/materials/viscous_fist_dissolve_vmat_g_tcolor_296284fc.vtex_c`
  - `models/heroes_staging/viscous/materials/viscous_ball_vmat_g_tcolor_2c347bde.vtex_c`
  - `models/abilities/materials/viscous_cube_color_png_81d0eb6a.vtex_c`
  - `models/abilities/materials/viscous_cube_color_png_85c8b349.vtex_c`
  - `models/abilities/materials/viscous_cube_color_png_daff99b9.vtex_c`
  - `models/abilities/materials/viscous_sphere_color_png_4de0c542.vtex_c`
  - `models/abilities/materials/viscous_fist_color_psd_ab531623.vtex_c`
  - `models/abilities/materials/viscous_fist_color_psd_d8e8086a.vtex_c`
- Key materials:
  - `models/abilities/materials/viscous_slime.vmat_c`
  - `models/abilities/materials/viscous_slime_blobs.vmat_c`
  - `models/abilities/materials/viscous_cube.vmat_c`
  - `models/heroes_staging/viscous/materials/viscous_punch.vmat_c`
  - `models/heroes_staging/viscous/materials/viscous_fist_dissolve.vmat_c`
  - `models/heroes_staging/viscous/materials/viscous_ball.vmat_c`

Generation notes:

- The prism bake reported that some VMATs had no patchable static tint, including:
  - `models/abilities/materials/viscous_cube.vmat_c`
  - `models/heroes_staging/viscous/materials/viscous_punch.vmat_c`
- Available texture paths and patchable material tints were still packed.

Still-risky Goo Ball model materials:

- `models/heroes_staging/viscous/materials/viscous_swatches.vmat_c`
- `models/heroes_staging/viscous/materials/viscous_outline.vmat_c`
- `models/heroes_staging/viscous/materials/black.vmat_c`
- `models/heroes_staging/viscous/materials/viscous_swatches_vmat_g_tcolor_3e195d73.vtex_c`
- `models/heroes_staging/viscous/materials/viscous_outline_vmat_g_tcolor_a3e220e4.vtex_c`
- `models/heroes_staging/viscous/materials/black_vmat_g_tcolor_43d4aef2.vtex_c`

Reason for caution:

- These come from `models/heroes_staging/viscous/viscous_inflated.vmdl_c`
  draw calls. They may be part of the hero body/outline rather than only the
  Goo Ball ability. Add only if in-game testing shows the ball still has green
  body sections after the `viscous_ball` texture/material fix.

## Celeste / Unicorn

User report:

- Celeste third ability also has a ground issue.

What was wrong:

- Celeste was still particle-only in the recipe, but her ground projections use
  hero-specific projected self-illum textures.
- The ability-3 ground particle is:
  `particles/abilities/unicorn/unicorn_radiant_blast_ground_projected.vpcf_c`
- It draws:
  `materials/particle/projected/unicorn_radiant_flare_ground_projected.vmat_c`
- That material uses:
  `materials/particle/projected/unicorn_radiant_flare_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`

Texture paths added:

- `materials/particle/abilities/unicorn/unicorn_prismatic_shield_ground_warning_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/unicorn_beams_of_light_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/unicorn_flux_rainbow_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/unicorn_radiant_flare_ground_advance_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/unicorn_radiant_flare_ground_preview_projected_vmat_g_tselfillum_670d93d.vtex_c`
- `materials/particle/projected/unicorn_radiant_flare_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`

Fix:

- Celeste now has a texture recipe instead of particle-only.
- Animated prism uses projected-texture rainbow for the radiant-flare ground
  textures, preserving authored falloff and scaling value to `0.72`.
- A test VPK was installed as:
  `zz_rainbow_prism_celeste_dir.vpk`
  so it sorts after existing Celeste cosmetic/particle packs.

## Current Installed Test Outputs

Affected QA-pass VPKs regenerated and installed:

- `pak02_dir.vpk` - Grey Talon / Archer
- `pak03_dir.vpk` - Bebop
- `pak04_dir.vpk` - Mo & Krill / Digger
- `pak07_dir.vpk` - Dynamo
- `pak09_dir.vpk` - Victor / Frank
- `pak10_dir.vpk` - Haze
- `pak19_dir.vpk` - Viscous
- `zz_rainbow_prism_celeste_dir.vpk` - Celeste / Unicorn

Dev Mode was left off. Installs were written to the normal Deadlock addons
folder and Grimoire metadata hashes were refreshed.

## Next Recommended Reviews

1. Grey Talon ultimate: check whether the owl body/eyes are still vanilla. If
   yes, review the `grey_talon_owl_*` VMAT/VTEX candidates carefully.
2. Dynamo edges: if ult/stomp/heal borders still lack contrast, add targeted
   high-visibility overrides for `dynamo_singularity_*_edge`,
   `dynamo_gravity_wave_rings`, and `dynamo_heal_buff_aoe_preview_band`.
3. Viscous Goo Ball: if any ball body/outline remains green, review
   `viscous_swatches`, `viscous_outline`, and `black` material candidates from
   `viscous_inflated.vmdl_c`.
4. Victor: if unicolor circles remain, identify which of the shared ground
   materials is visible in-game and handle it with particle/material special
   logic, not shared texture recolor.
5. Celeste: verify ability 3 specifically with the `zz_` VPK enabled after
   existing Celeste packs.

## Full Roster Re-Audit

Date: 2026-06-01

Scope:

- All current `pinned_hero_codenames()` recipes in `vpkmerge-core/src/hero_recolor.rs`
- Base VPK: `/home/esoc/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`

Checks run:

- `cargo test -p vpkmerge-core hero_recolor --lib`
- `cargo run -p vpkmerge-cli -- rainbow-scan --vpk /home/esoc/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`
- `target/debug/vpkmerge prism --animated --hero <codename> --vpk <pak01_dir.vpk> --encode-vpk /tmp/vpkmerge-prism-audit/current_dir.vpk` for every pinned hero

Code-level result:

- All `hero_recolor` tests passed.
- `rainbow-scan` completed for every pinned hero.
- Every animated prism bake completed.
- No missing recipe entries were found.
- No particle patch errors were found.
- No material patch errors were found.

Bake-time notes that still matter:

- `frank`: `materials/particle/abilities/frank/frank_painaura_sphere.vmat_c`
  is still included as a reviewed candidate, but it reports no patchable prism
  material tint.
- `viscous`: `models/abilities/materials/viscous_cube.vmat_c` and
  `models/heroes_staging/viscous/materials/viscous_punch.vmat_c` are still
  included as reviewed candidates, but report no patchable prism material tint.
  Viscous still patches 14 textures and 4 other material tints.

Obvious remaining work:

1. Grey Talon owl body/eyes still need in-game confirmation before adding the
   risky `grey_talon_owl_*` body/eye materials.
2. Dynamo ult/stomp/heal edges still need in-game contrast confirmation. If
   weak, add targeted high-visibility particle overrides rather than shared
   texture recolors.
3. Viscous Goo Ball still needs in-game confirmation for body/outline sections.
   If green remains, review `viscous_swatches`, `viscous_outline`, and `black`
   from `viscous_inflated.vmdl_c`.
4. Victor shared ground material leftovers still need visual identification if
   unicolor circles remain. Do not add shared ground textures directly.
5. Celeste ability 3 still needs load-order/in-game verification with the `zz_`
   VPK active after existing Celeste packs.
6. Infernus fire textures remain a known architecture limitation: they are
   shared game-wide, so a full texture recolor needs copy-and-repoint support
   rather than in-place shared texture edits.
