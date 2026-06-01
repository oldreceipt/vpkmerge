# Remaining Roster Recolor Audit

Local audit inputs:

- Base VPK: `~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`
- File tree dump: `.scratch/scans/pak01_filetree_2026-05-31.txt`
- Roster metadata dump: `.scratch/scans/hero_roster_audit_2026-05-31.txt`
- Ability texture scans: `.scratch/scans/recolor_assets_2026-05-31/*.txt`
- Local mod scan reports: `.scratch/scans/local_mods/*.txt`

## Local Mods

The checked-in mod VPKs under `mods/` are texture/skin replacements, not ability
VFX recolors. `particle_scan` found no `particles/abilities/<hero>/` or
`particles/weapon_fx/<hero>/` entries in the Yamato/Vindicta mod VPKs, so they do
not contribute additional ability recipes.

## Base Roster

`scripts/heroes.vdata_c` plus the `particles/abilities` and `particles/weapon_fx`
tree were used as the source of truth. The newly integrated selectable namespaces
are:

`archer`, `bebop`, `digger`, `doorman`, `drifter`, `dynamo`, `familiar`,
`frank`, `haze`, `hornet`, `kelvin`, `mirage`, `priest`, `punkgoat`, `shiv`,
`tengu`, `viper`, `viscous`, `warden`, and `werewolf`.

This brings the built-in recipe table in line with the current selectable
roster namespaces from the local `pak01`.

## Texture Decisions

The recipe additions use the standard particle roots:

- `particles/abilities/<codename>/`
- `particles/weapon_fx/<codename>/`

The texture entries are only included when the ability scan found hero-specific
chromatic maps, or when a manually reviewed alias path is ability-specific:

- `tengu` keeps the particle namespace but includes Ivy-era ability maps:
  `ivy_vine_cable` and `ivy_entangling_thorns`.
- `kelvin` includes the ice-dome model color map, but excludes the broadly shared
  `ice_surface` texture.
- `warden` includes the shield scanline texture, but excludes the body albedo
  referenced by `warden_temp.vmat_c`, since overriding it would recolor the skin.
- Generic/shared defaults, noise, ground, and cross-hero textures are left out.

Particle-only additions are `bebop`, `hornet`, `mirage`, `punkgoat`, and `shiv`.
Their audited ability materials did not expose safe hero-specific chromatic
textures, so the particle color fields carry the recolor.

## Verification

Commands run:

```bash
cargo test -p vpkmerge-core
cargo test -p vpkmerge-gui
cargo run --release -p vpkmerge-cli -- rainbow-scan --vpk "$PAK" --hero ...
```

Every newly added namespace was also preflight-baked with:

```bash
cargo run --release -p vpkmerge-cli -- recolor-hero \
  --hero <codename> --vpk "$PAK" --hue 280 \
  --encode-vpk ".scratch/preflight_roster_2026-05-31/<codename>_h280_dir.vpk"
```

All new preflight bakes completed with zero unpatchable particles.
