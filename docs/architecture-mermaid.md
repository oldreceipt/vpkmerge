# vpkmerge Mermaid Architecture

This document is a Mermaid-first map of the current repository architecture and
the Deadlock hero recolor recipe surface.

Source files used as the source of truth:

- `Cargo.toml`
- `vpkmerge-core/src/lib.rs`
- `vpkmerge-core/src/hero_recolor.rs`
- `vpkmerge-core/src/recolor.rs`
- `vpkmerge-core/src/model.rs`
- `vpkmerge-core/src/portrait.rs`
- `vpkmerge-core/src/soundevents.rs`
- `vpkmerge-core/src/trippy.rs`
- `morphic/src/lib.rs`
- `gui/src-tauri/src/lib.rs`
- `vpkmerge-cli/src/main.rs`

## Workspace Architecture

```mermaid
flowchart LR
    subgraph Workspace["Cargo workspace"]
        CLI["vpkmerge-cli<br/>clap command binary"]
        GUIFE["gui/src<br/>Vue 3 + Vite frontend"]
        GUIBE["gui/src-tauri<br/>Tauri v2 Rust backend"]
        CORE["vpkmerge-core<br/>library orchestration"]
        MORPHIC["morphic<br/>Source 2 resource codec"]
    end

    CLI --> CORE
    GUIFE -->|"invoke commands"| GUIBE
    GUIBE --> CORE
    GUIBE -->|"texture previews"| MORPHIC
    CORE --> MORPHIC

    CORE --> VALVEPAK["valve_pak<br/>VPK open, read, pack"]
    CORE --> TEMPFILE["tempfile<br/>staging directories"]
    CORE --> SERDEJSON["serde_json<br/>soundevents JSON projection"]

    MORPHIC --> RESOURCE["Source 2 resource container"]
    MORPHIC --> TEXTURE["VTEX decode/encode/edit"]
    MORPHIC --> KV3["binary KV3 decode/write/patch"]
    MORPHIC --> MODEL["VMDL decode/edit/GLB"]
    MORPHIC --> MATERIAL["VMAT PBR/tint parse"]
```

## Crate Boundaries

```mermaid
flowchart TB
    subgraph Frontends
        CLI_MERGE["CLI: merge default command"]
        CLI_SPLIT["CLI: split"]
        CLI_ASSET["CLI: portrait / model / soundevents / texture"]
        CLI_RECOLOR["CLI: recolor-hero / prism / trippy-* / rainbow-scan"]
        GUI_MERGE["GUI Browse/Merge tabs"]
        GUI_PRISM["GUI Prism tab"]
        GUI_LOCKER["GUI Locker tab"]
    end

    subgraph Core["vpkmerge-core public modules"]
        CORE_LIB["lib.rs<br/>merge, split, pack, read_vpk_entry"]
        CORE_PORTRAIT["portrait.rs<br/>hero portrait extraction"]
        CORE_SOUND["soundevents.rs<br/>vsndevts edit/encode"]
        CORE_MODEL["model.rs<br/>VMDL inspect/export/edit/recolor"]
        CORE_RECOLOR["recolor.rs<br/>texture + vertex COLOR recolor"]
        CORE_HERO["hero_recolor.rs<br/>pinned VFX recipes + prism"]
        CORE_TRIPPY["trippy.rs<br/>procedural skin/VFX painting"]
    end

    subgraph Morphic["morphic internals"]
        M_RESOURCE["resource/<br/>container header + block table"]
        M_TEXTURE["texture/<br/>format, decode, encode, mip splice"]
        M_KV3["kv3/<br/>reader, writer, patch, rewrap"]
        M_MATERIAL["material/<br/>VMAT texture/tint/PBR slots"]
        M_MODEL["model/<br/>mesh, skeleton, animation, NM, edit, GLB"]
        M_MESHOPT["meshopt/<br/>compressed vertex/index buffers"]
    end

    CLI_MERGE --> CORE_LIB
    CLI_SPLIT --> CORE_LIB
    CLI_ASSET --> CORE_PORTRAIT
    CLI_ASSET --> CORE_SOUND
    CLI_ASSET --> CORE_MODEL
    CLI_ASSET --> CORE_RECOLOR
    CLI_RECOLOR --> CORE_HERO
    CLI_RECOLOR --> CORE_TRIPPY

    GUI_MERGE --> CORE_LIB
    GUI_PRISM --> CORE_HERO
    GUI_LOCKER --> CORE_HERO
    GUI_LOCKER --> CORE_TRIPPY

    CORE_PORTRAIT --> M_TEXTURE
    CORE_SOUND --> M_KV3
    CORE_MODEL --> M_MODEL
    CORE_MODEL --> M_MATERIAL
    CORE_RECOLOR --> M_TEXTURE
    CORE_RECOLOR --> M_MODEL
    CORE_HERO --> M_KV3
    CORE_HERO --> M_TEXTURE
    CORE_HERO --> M_MODEL
    CORE_HERO --> M_MATERIAL
    CORE_TRIPPY --> M_KV3
    CORE_TRIPPY --> M_TEXTURE
    CORE_TRIPPY --> M_MODEL

    M_TEXTURE --> M_RESOURCE
    M_KV3 --> M_RESOURCE
    M_MATERIAL --> M_KV3
    M_MODEL --> M_RESOURCE
    M_MODEL --> M_KV3
    M_MODEL --> M_MATERIAL
    M_MODEL --> M_MESHOPT
```

## Main Data Flows

```mermaid
flowchart TB
    subgraph MergeSplit["Merge and split"]
        MERGE_IN["N input *_dir.vpk files"] --> INSPECT["valve_pak::open + file_paths"]
        INSPECT --> OWNERS["compute path owners"]
        OWNERS --> POLICY["collision policy + overrides"]
        POLICY --> STAGE["safe_join into temp directory"]
        STAGE --> PACK["valve_pak::from_directory"]
        PACK --> MERGE_OUT["output *_dir.vpk"]

        SPLIT_IN["1 input *_dir.vpk"] --> ROUTES["PathPredicate::AnyPrefix routes"]
        ROUTES --> BUCKETS["write_bucket per output + optional residual"]
        BUCKETS --> SPLIT_OUT["N output *_dir.vpk files"]
    end

    subgraph AssetTools["Asset tools"]
        PORTRAITS["portrait command"] --> PORTRAIT_FLOW["find panorama/images/heroes/*.vtex_c<br/>decode via morphic<br/>write PNG + manifest"]
        TEXTURE_CMD["texture command"] --> TEXTURE_FLOW["decode .vtex_c top mip<br/>set HSV hue/saturation/value<br/>replace full mip chain<br/>write file or pack VPK"]
        SOUND_CMD["soundevents command"] --> SOUND_FLOW["decode KV3 DATA<br/>edit vsnd paths or numeric fields<br/>encode resource<br/>write file or pack VPK"]
        MODEL_CMD["model command"] --> MODEL_FLOW["inspect/export/edit .vmdl_c<br/>resolve .vmat_c/.vtex_c across skin then base<br/>write GLB or addon VPK"]
    end
```

## Source 2 Codec Layer

```mermaid
flowchart TB
    BYTES["compiled Source 2 resource bytes"] --> RES["morphic::resource::Resource::parse"]
    RES --> DATA["DATA block"]
    RES --> NONDATA["non-DATA blocks preserved"]

    DATA --> VTEX["VTEX texture header + pixel data"]
    VTEX --> INSPECT_TEX["inspect / parse_texture_header"]
    VTEX --> DECODE_TEX["decode / decode_at"]
    VTEX --> REPLACE_MIP["replace_mip_chain / replace_face_mip_chain"]
    DECODE_TEX --> IMAGE["ImageData::Rgba8 or Rgba16F"]

    DATA --> KV3["binary KV3"]
    KV3 --> KV3_TREE["kv3::Value tree"]
    KV3_TREE --> ENCODE_KV3["encode_kv3_resource<br/>uncompressed v4 DATA"]
    KV3_TREE --> PATCH_KV3["patch scalars / doubles / floats / strings / array insert<br/>byte-faithful"]

    DATA --> VMDL["VMDL model resource"]
    VMDL --> MODEL_DECODE["model::decode"]
    MODEL_DECODE --> MODEL_PARTS["skeleton, meshes, materials, animations"]
    MODEL_PARTS --> GLB["to_glb_textured"]
    MODEL_PARTS --> MODEL_EDIT["geometry edit, part replace/remove, vertex COLOR recolor"]
```

## GUI Command Surface

```mermaid
flowchart LR
    VUE["Vue app"] --> BROWSE["BrowseTab.vue"]
    VUE --> PRISM["PrismTab.vue"]
    VUE --> LOCKER["LockerTab.vue"]

    BROWSE --> ADDMOD["add_mod"]
    BROWSE --> CONFLICTS["detect_conflicts"]
    BROWSE --> PREVIEW["preview_texture"]
    BROWSE --> MERGE["merge_vpks"]

    PRISM --> HERO_OPTS["supported_hero_options"]
    PRISM --> HERO_PARTS["hero_recipe_parts"]
    PRISM --> BUILD_PRISM["build_hero_prism_vpk"]

    LOCKER --> STYLE_OPTS["trippy_style_options"]
    LOCKER --> ANIM_OPTS["trippy_animation_options"]
    LOCKER --> TRIPPY_PREVIEW["trippy_preview"]
    LOCKER --> BUILD_TRIPPY["build_trippy_addon"]

    ADDMOD --> CORE_INSPECT["vpkmerge_core::inspect"]
    CONFLICTS --> CORE_CONFLICTS["vpkmerge_core::detect_conflicts"]
    MERGE --> CORE_MERGE["vpkmerge_core::merge"]
    PREVIEW --> MORPHIC_PREVIEW["valve_pak + morphic::inspect/decode_at + PNG data URL"]
    HERO_OPTS --> RECIPES["pinned_hero_codenames + recipe_for"]
    HERO_PARTS --> RECIPES
    BUILD_PRISM --> PRISM_CORE["prism_recolor_hero_to_addon"]
    BUILD_TRIPPY --> TRIPPY_CORE["trippy_skin_to_addon / trippy_ability_vfx_to_addon"]
```

## Hero Recolor Pipeline

```mermaid
flowchart TB
    START["recolor_hero_to_addon<br/>or prism_recolor_hero_to_addon_tuned"] --> RECIPE["recipe_for(codename)<br/>HeroRecolorRecipe"]
    RECIPE --> OPEN["open_vpks<br/>skin VPK first, base VPK second"]

    OPEN --> PARTICLE_LIST["list_entries(recipe.particle_prefixes, .vpcf_c)<br/>deduped + sorted"]
    PARTICLE_LIST --> PARTICLE_READ["read_entry first matching VPK"]
    PARTICLE_READ --> PARTICLE_DECODE["morphic::decode_kv3_resource"]
    PARTICLE_DECODE --> PARTICLE_HUE["recolor_particle_bytes<br/>collect color/tint Color32 edits"]
    PARTICLE_DECODE --> PARTICLE_PRISM["prism_recolor_particle_bytes<br/>gradient + color field spectrum"]
    PARTICLE_PRISM --> PARTICLE_ANIM["optional animation<br/>timing patch, loop gradients, insert color-cycle operator"]
    PARTICLE_HUE --> PATCH_SCALAR["morphic::patch_kv3_resource_scalars"]
    PARTICLE_PRISM --> PATCH_SCALAR
    PARTICLE_ANIM --> PATCH_STRUCT["patch strings / floats / array insert"]

    OPEN --> TEXTURES["recipe.texture_entries"]
    TEXTURES --> TEX_READ["read_entry"]
    TEX_READ --> TEX_RECOLOR["recolor_texture_hue<br/>or prism_recolor_texture_bytes"]
    TEX_RECOLOR --> TEX_MORPHIC["morphic decode top mip<br/>HSV transform or rainbow bands<br/>replace_mip_chain"]

    OPEN --> MATERIALS["recipe.material_entries"]
    MATERIALS --> MAT_READ["read_entry"]
    MAT_READ --> MAT_RECOLOR["recolor_material_color_bytes<br/>stamp g_vSelfIllumTint / eligible g_vColorTint"]
    MAT_RECOLOR --> MAT_PATCH["morphic::patch_kv3_resource_doubles<br/>or safe KV3 re-encode"]

    OPEN --> MODELS["recipe.model_entries"]
    MODELS --> MODEL_READ["read_entry"]
    MODEL_READ --> MODEL_RECOLOR["recolor_model_vertex_colors"]
    MODEL_RECOLOR --> MODEL_PATCH["morphic::model::vertex_targets<br/>recolor COLOR lanes<br/>re-encode vertex buffer"]

    PATCH_SCALAR --> PACKED["packed entries"]
    PATCH_STRUCT --> PACKED
    TEX_MORPHIC --> PACKED
    MAT_PATCH --> PACKED
    MODEL_PATCH --> PACKED
    PACKED --> PACK_ADDON["vpkmerge_core::pack"]
    PACK_ADDON --> OUT["single addon *_dir.vpk overriding base paths"]
```

## Hero Recolor Coverage

```mermaid
flowchart TB
    ROOT["Pinned hero recolor recipes<br/>vpkmerge-core/src/hero_recolor.rs"] --> PARTICLE_ONLY["Particles only"]
    ROOT --> PARTICLE_TEXTURE["Particles + explicit .vtex_c textures"]
    ROOT --> MATERIAL_TINT["Also .vmat_c tint constants"]
    ROOT --> MODEL_COLOR["Also .vmdl_c vertex COLOR"]
    ROOT --> SPECIAL_PRISM["Special prism handling"]

    PARTICLE_ONLY --> astro["astro / Holliday"]
    PARTICLE_ONLY --> bebop["bebop"]
    PARTICLE_ONLY --> gigawatt["gigawatt / Seven"]
    PARTICLE_ONLY --> hornet["hornet / Vindicta"]
    PARTICLE_ONLY --> inferno["inferno / Infernus"]
    PARTICLE_ONLY --> mirage["mirage"]
    PARTICLE_ONLY --> punkgoat["punkgoat / Billy"]
    PARTICLE_ONLY --> shiv["shiv"]
    PARTICLE_ONLY --> vampirebat["vampirebat / Mina"]
    PARTICLE_ONLY --> wraith["wraith"]

    PARTICLE_TEXTURE --> abrams["abrams"]
    PARTICLE_TEXTURE --> archer["archer / Grey Talon"]
    PARTICLE_TEXTURE --> bookworm["bookworm / Paige"]
    PARTICLE_TEXTURE --> chrono["chrono / Paradox"]
    PARTICLE_TEXTURE --> digger["digger / Mo and Krill"]
    PARTICLE_TEXTURE --> doorman["doorman"]
    PARTICLE_TEXTURE --> drifter["drifter"]
    PARTICLE_TEXTURE --> dynamo["dynamo"]
    PARTICLE_TEXTURE --> familiar["familiar"]
    PARTICLE_TEXTURE --> fencer["fencer / Apollo"]
    PARTICLE_TEXTURE --> frank["frank / Victor"]
    PARTICLE_TEXTURE --> ghost["ghost / Lady Geist"]
    PARTICLE_TEXTURE --> haze["haze"]
    PARTICLE_TEXTURE --> kelvin["kelvin"]
    PARTICLE_TEXTURE --> lash["lash"]
    PARTICLE_TEXTURE --> magician["magician / Sinclair"]
    PARTICLE_TEXTURE --> mcginnis["mcginnis"]
    PARTICLE_TEXTURE --> nano["nano / Calico"]
    PARTICLE_TEXTURE --> necro["necro / Graves"]
    PARTICLE_TEXTURE --> pocket["pocket"]
    PARTICLE_TEXTURE --> priest["priest"]
    PARTICLE_TEXTURE --> tengu["tengu / Ivy"]
    PARTICLE_TEXTURE --> unicorn["unicorn / Celeste"]
    PARTICLE_TEXTURE --> viper["viper / Vyper"]
    PARTICLE_TEXTURE --> viscous["viscous"]
    PARTICLE_TEXTURE --> warden["warden"]
    PARTICLE_TEXTURE --> werewolf["werewolf"]
    PARTICLE_TEXTURE --> yamato["yamato"]

    MATERIAL_TINT --> dynamo
    MATERIAL_TINT --> frank
    MATERIAL_TINT --> necro
    MATERIAL_TINT --> viscous

    MODEL_COLOR --> bookworm

    SPECIAL_PRISM --> bebop_passthrough["bebop projected laser decals stay vanilla"]
    SPECIAL_PRISM --> yamato_bands["yamato shadow status textures get rainbow bands when animated"]
    SPECIAL_PRISM --> yamato_muted["yamato shadow shape texture gets muted blue prism hue"]
    SPECIAL_PRISM --> projected_bands["animated projected texture rainbow bands:<br/>digger, frank, dynamo, unicorn"]
```

## Particle Prefix Map

Every pinned hero touches all `.vpcf_c` files under its particle prefixes. Entries
are discovered at bake time from the input VPK(s), de-duplicated, and read from
skin VPK first, then base VPK.

```mermaid
flowchart TB
    STD["standard roots<br/>particles/abilities/&lt;codename&gt;/<br/>particles/weapon_fx/&lt;codename&gt;/"] --> STD_HEROES["abrams, archer, astro, bebop, bookworm, chrono, digger, doorman, drifter, familiar, fencer, frank, ghost, gigawatt, haze, hornet, inferno, kelvin, lash, magician, mcginnis, mirage, nano, pocket, priest, punkgoat, shiv, tengu, unicorn, vampirebat, viper, warden, werewolf, wraith"]
    STD --> DYN["dynamo"]
    STD --> NEC["necro"]
    STD --> VIS["viscous"]
    STD --> YAM["yamato"]

    DYN --> DYN_EXTRA1["particles/dynamo/"]
    DYN --> DYN_EXTRA2["particles/status_fx/status_fx_dynamo"]
    NEC --> NEC_EXTRA["particles/heroes/necro/"]
    VIS --> VIS_EXTRA["particles/abilities/melee/viscous/"]
    YAM --> YAM_EXTRA["particles/status_fx/status_fx_yamato"]
```

## File-Level Texture Map 1

```mermaid
flowchart TB
    bookworm["bookworm"] --> bw_t1["materials/particle/abilities/bookworm/bookworm_projectile_self_illum_vmat_g_tcolor_7b26a19f.vtex_c"]
    bookworm --> bw_t2["materials/particle/projected/bookworm_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    bookworm --> bw_t3["materials/particle/ground/ground_streak_bookworm_psd_5a44028c.vtex_c"]
    bookworm --> bw_t4["models/heroes_wip/bookworm/materials/bookworm_ui_effects_color_psd_a29be817.vtex_c"]
    bookworm --> bw_t5["models/heroes_wip/bookworm/materials/bookworm_shield_illustrated_color_psd_81f5497b.vtex_c"]
    bookworm --> bw_t6["models/heroes_wip/bookworm/materials/bookworm_sword_illustrated_color_psd_4eb22603.vtex_c"]
    bookworm --> bw_t7["models/heroes_wip/bookworm/materials/bookworm_stone_illustrated_color_psd_8ed29960.vtex_c"]
    bookworm --> bw_t8["models/heroes_wip/bookworm/materials/bookworm_dragon_color_tga_ed3d3b5.vtex_c"]
    bookworm --> bw_t9["materials/models/particle/bookworm/neutral_black_dragon_color_psd_b8c8249f.vtex_c"]

    abrams["abrams"] --> ab_t1["materials/particle/abilities/abrams/abrams_leap_ground_impact_hot_symbol_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    abrams --> ab_t2["materials/particle/projected/abrams_siphon_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]

    archer["archer"] --> ar_t1["materials/particle/abilities/archer/archer_charged_shot_gradient_color_v2_psd_51e62704.vtex_c"]
    archer --> ar_t2["materials/particle/abilities/archer/archer_charged_shot_gradient_color_psd_17c02a47.vtex_c"]
    archer --> ar_t3["materials/particle/abilities/archer_guided_arrow_explosion_sphere_vmat_g_tcolor_a84e2808.vtex_c"]
    archer --> ar_t4["materials/models/particle/archer/archer_arrow_illum_vmat_g_tcolor_7d46cca1.vtex_c"]
    archer --> ar_t5["models/heroes_staging/archer/materials/archer_guided_arrow_color_psd_edd3d0f5.vtex_c"]
    archer --> ar_t6["models/heroes_staging/archer/bird/materials/bird_color_psd_117d09e0.vtex_c"]

    chrono["chrono"] --> chr_t1["models/heroes_staging/chrono/materials/chrono_fx_bubble02_color_psd_f57b1ef0.vtex_c"]
    chrono --> chr_t2["models/heroes_staging/chrono/materials/chrono_fx_bubble04_color_psd_ee26af5c.vtex_c"]

    digger["digger"] --> dig_t1["materials/particle/abilities/digger/digger_burrow_channel_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    digger --> dig_t2["materials/particle/abilities/digger/digger_burrow_explode_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    digger --> dig_t3["materials/particle/abilities/digger/digger_burrow_spin_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c"]

    doorman["doorman"] --> door_t1["materials/particle/abilities/doorman/doorman_grenade_debuff_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    drifter["drifter"] --> drift_t1["materials/particle/projected/drifter_claw_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
```

## File-Level Texture Map 2

```mermaid
flowchart TB
    dynamo["dynamo"] --> dyn_t1["materials/particle/projected/dynamo_void_sphere_projected_ground_vmat_g_tselfillum_670d93d.vtex_c"]
    dynamo --> dyn_t2["materials/particle/abilities/dynamo/dynamo_void_sphere_marker_symbol_psd_73d6401e.vtex_c"]
    dynamo --> dyn_t3["materials/particle/abilities/dynamo/dynamo_void_sphere_planet_symbol.vtex_c"]
    dynamo --> dyn_t4["materials/particle/abilities/dynamo/dynamo_void_sphere_sun_halo_symbol.vtex_c"]
    dynamo --> dyn_t5["materials/particle/abilities/dynamo/dynamo_void_sphere_sun_symbol.vtex_c"]

    familiar["familiar"] --> fam_t1["materials/particle/abilities/familiar/familiar_naptime_coneradius_intersection_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    familiar --> fam_t2["materials/particle/abilities/familiar/familiar_pillow_explode_ground_bright_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    familiar --> fam_t3["materials/particle/abilities/familiar/familiar_pillow_explode_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    familiar --> fam_t4["materials/particle/abilities/familiar/familiar_spotlight_ground_edge_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    familiar --> fam_t5["materials/particle/abilities/familiar/familiar_spotlight_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]

    fencer["fencer"] --> fen_t1["materials/particle/projected/fencer_preview_line_projected_decal_vmat_g_tselfillum_670d93d.vtex_c"]
    fencer --> fen_t2["materials/particle/projected/fencer_sigil_pentagram_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    fencer --> fen_t3["materials/particle/abilities/fencer/fencer_ult_gradient_color_psd_51322651.vtex_c"]
    fencer --> fen_t4["models/heroes_wip/fencer/materials/fencer_sword_color_tga_52ec8bfe.vtex_c"]

    frank["frank"] --> fr_t1["materials/particle/abilities/frank/frank_painaura_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    frank --> fr_t2["materials/particle/abilities/frank/frank_revive_marker_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    frank --> fr_t3["materials/particle/projected/frank_shock_miss_projected_bright_vmat_g_tselfillum_670d93d.vtex_c"]

    ghost["ghost"] --> gh_t1["models/heroes_staging/ghost/materials/ghost2_clothes_fx_prop_color_psd_b398de35.vtex_c"]
    ghost --> gh_t2["models/heroes_staging/ghost/materials/ghost2_clothes_color_png_fc80b39a.vtex_c"]

    haze["haze"] --> haz_t1["materials/particle/abilities/haze/haze_tracer_self_illum_vmat_g_tcolor_52a5b2da.vtex_c"]

    kelvin["kelvin"] --> kel_t1["materials/particle/projected/kelvin_ice_dome_projected_psd_d86c1818.vtex_c"]
    kelvin --> kel_t2["materials/particle/projected/kelvin_ice_dome_projected_psd_b5785889.vtex_c"]
    kelvin --> kel_t3["models/abilities/materials/ice_dome_color_psd_3a38e562.vtex_c"]

    lash["lash"] --> lash_t1["materials/particle/cables/lash_cable_material_vmat_g_tcolor_8ca8af3e.vtex_c"]

    magician["magician"] --> mag_t1["materials/particle/projected/magician_hex_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    magician --> mag_t2["materials/particle/abilities/magician/magician_bolt_vmat_g_tcolor_978bc798.vtex_c"]

    mcginnis["mcginnis"] --> mcg_t1["materials/particle/abilities/mcginnis/mcginnis_turret_ambient_goo_vmat_g_tcolor_974c5f09.vtex_c"]
    mcginnis --> mcg_t2["materials/particle/abilities/mcginnis/mcginnis_turret_ambient_goo_vmat_g_tsheen_7edd324d.vtex_c"]

    nano["nano"] --> nano_t1["materials/particle/abilities/nano/nano_ult_ground_dark_proj_vmat_g_tselfillum_670d93d.vtex_c"]
    nano --> nano_t2["models/heroes_staging/nano/cat_statue/materials/cat_statue_color_png_8892a790.vtex_c"]

    pocket["pocket"] --> poc_t1["materials/particle/projected/pocket_satchel_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    pocket --> poc_t2["models/heroes_staging/synth/materials/pocket_body_color_png_eb808d8a.vtex_c"]
    pocket --> poc_t3["materials/particle/abilities/pocket/pocket_magic_missile_illum_vmat_g_tcolor_754e94bd.vtex_c"]
    pocket --> poc_t4["models/heroes_staging/synth/materials/pocket_suitcase_vmat_g_tcolor_e71e9d59.vtex_c"]
    pocket --> poc_t5["models/abilities/materials/pocket_frog_small_color_png_e2620619.vtex_c"]
    pocket --> poc_t6["models/abilities/materials/synth_deployable_color_psd_a57da819.vtex_c"]

    priest["priest"] --> pri_t1["materials/particle/abilities/priest/priest_flashbang_debuff_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    priest --> pri_t2["materials/particle/abilities/priest/priest_snaptrap_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    priest --> pri_t3["materials/particle/projected/priest_snaptrap_projectile_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
```

## File-Level Texture Map 3

```mermaid
flowchart TB
    tengu["tengu"] --> ten_t1["materials/particle/cables/ivy_vine_cable_vmat_g_tcolor_9509ed42.vtex_c"]
    tengu --> ten_t2["models/abilities/materials/ivy_entangling_thorns_vmat_g_tcolor_59ac0039.vtex_c"]

    unicorn["unicorn"] --> uni_t1["materials/particle/abilities/unicorn/unicorn_prismatic_shield_ground_warning_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    unicorn --> uni_t2["materials/particle/projected/unicorn_beams_of_light_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c"]
    unicorn --> uni_t3["materials/particle/projected/unicorn_flux_rainbow_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c"]
    unicorn --> uni_t4["materials/particle/projected/unicorn_radiant_flare_ground_advance_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    unicorn --> uni_t5["materials/particle/projected/unicorn_radiant_flare_ground_preview_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    unicorn --> uni_t6["materials/particle/projected/unicorn_radiant_flare_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]

    viper["viper"] --> vip_t1["materials/particle/abilities/viper/viper_petrify_symbol_ground_psd_a643967f.vtex_c"]

    viscous["viscous"] --> vis_t1["materials/models/particle/viscous_puddle_telegraph_vmat_g_tcolor_ac749641.vtex_c"]
    viscous --> vis_t2["materials/particle/abilities/viscous/viscous_detail_psd_a2817163.vtex_c"]
    viscous --> vis_t3["materials/particle/abilities/viscous/viscous_detail_psd_3c03ec04.vtex_c"]
    viscous --> vis_t4["materials/particle/abilities/viscous/viscous_detail_psd_4414414e.vtex_c"]
    viscous --> vis_t5["models/heroes_staging/viscous/materials/viscous_punch_preview_vmat_g_tcolor_32414205.vtex_c"]
    viscous --> vis_t6["models/heroes_staging/viscous/materials/viscous_punch_vmat_g_tcolor_afc99362.vtex_c"]
    viscous --> vis_t7["models/heroes_staging/viscous/materials/viscous_fist_dissolve_vmat_g_tcolor_296284fc.vtex_c"]
    viscous --> vis_t8["models/heroes_staging/viscous/materials/viscous_ball_vmat_g_tcolor_2c347bde.vtex_c"]
    viscous --> vis_t9["models/abilities/materials/viscous_cube_color_png_81d0eb6a.vtex_c"]
    viscous --> vis_t10["models/abilities/materials/viscous_cube_color_png_85c8b349.vtex_c"]
    viscous --> vis_t11["models/abilities/materials/viscous_cube_color_png_daff99b9.vtex_c"]
    viscous --> vis_t12["models/abilities/materials/viscous_sphere_color_png_4de0c542.vtex_c"]
    viscous --> vis_t13["models/abilities/materials/viscous_fist_color_psd_ab531623.vtex_c"]
    viscous --> vis_t14["models/abilities/materials/viscous_fist_color_psd_d8e8086a.vtex_c"]

    warden["warden"] --> war_t1["materials/models/particle/warden_tech_shield_scanline_color_psd_7e04e0b4.vtex_c"]

    werewolf["werewolf"] --> wolf_t1["materials/particle/abilities/werewolf/werewolf_cripplingslash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    werewolf --> wolf_t2["materials/particle/projected/werewolf_transform_bite_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    werewolf --> wolf_t3["materials/particle/projected/werewolf_transform_crushing_leap_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]

    yamato["yamato"] --> yam_t1["materials/particle/projected/yamato_blade_dash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c"]
    yamato --> yam_t2["materials/particle/abilities/yamato/yamato_shadow_redemption_complete_status.vtex_c"]
    yamato --> yam_t3["materials/particle/abilities/yamato/yamato_shadow_redemption_nokill_status.vtex_c"]
    yamato --> yam_t4["models/heroes_staging/yamato_v2/materials/yamoto_shadow_shape_color_psd_fe3c64a6.vtex_c"]

    necro["necro"] --> nec_t1["models/heroes_wip/necro/materials/necro_shambler_color_tga_7b1de566.vtex_c"]
    necro --> nec_t2["models/heroes_wip/necro/materials/necro_shambler_vmat_g_tnprtransmissivecolor_337e62d.vtex_c"]
    necro --> nec_t3["models/heroes_wip/necro/materials/necro_jar_of_dread_color_tga_7f34b26.vtex_c"]
    necro --> nec_t4["models/heroes_wip/necro/materials/necro_jar_glass_color_tga_c6d5a0ec.vtex_c"]
    necro --> nec_t5["models/heroes_wip/necro/materials/necro_gravestone_color_tga_8a0745c.vtex_c"]
    necro --> nec_t6["models/heroes_wip/necro/materials/necro_gravestone_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c"]
    necro --> nec_t7["models/abilities/materials/necro_gravestone_destruction_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c"]
    necro --> nec_t8["models/heroes_wip/necro/materials/necro_hand_color_tga_b2300f7f.vtex_c"]
    necro --> nec_t9["models/heroes_wip/necro/materials/necro_hand_vmat_g_tnprtransmissivecolor_c987b5a.vtex_c"]
```

## Material and Model Extras

```mermaid
flowchart TB
    dynamo["dynamo"] --> dyn_m1["materials/models/particle/dynamo_void_sphere_cyl.vmat_c"]
    dynamo --> dyn_m2["materials/models/particle/dynamo_heal_buff_model.vmat_c"]

    frank["frank"] --> frank_m1["materials/particle/abilities/frank/frank_painaura_sphere.vmat_c"]

    viscous["viscous"] --> vis_m1["models/abilities/materials/viscous_slime.vmat_c"]
    viscous --> vis_m2["models/abilities/materials/viscous_slime_blobs.vmat_c"]
    viscous --> vis_m3["models/abilities/materials/viscous_cube.vmat_c"]
    viscous --> vis_m4["models/heroes_staging/viscous/materials/viscous_punch.vmat_c"]
    viscous --> vis_m5["models/heroes_staging/viscous/materials/viscous_fist_dissolve.vmat_c"]
    viscous --> vis_m6["models/heroes_staging/viscous/materials/viscous_ball.vmat_c"]

    necro["necro"] --> nec_m1["models/abilities/materials/necro_pickup_sphere.vmat_c"]
    necro --> nec_m2["materials/particle/abilities/necro/necro_jar_glass.vmat_c"]
    necro --> nec_m3["models/abilities/materials/necro_hands.vmat_c"]
    necro --> nec_m4["models/heroes_wip/necro/materials/necro_flame_effect_hand.vmat_c"]
    necro --> nec_m5["models/heroes_wip/necro/materials/necro_flame_effect.vmat_c"]
    necro --> nec_m6["models/heroes_wip/necro/materials/necro_picker_hand_effect.vmat_c"]
    necro --> nec_m7["models/heroes_wip/necro/materials/necro_picker_effect.vmat_c"]
    necro --> nec_m8["models/heroes_wip/necro/materials/picker_hand_glow.vmat_c"]
    necro --> nec_m9["models/heroes_wip/necro/materials/necro_gravestone.vmat_c"]
    necro --> nec_m10["models/abilities/materials/necro_gravestone_destruction.vmat_c"]

    bookworm["bookworm"] --> bw_model1["models/particle/bookworm_horse_knight.vmdl_c"]
    bookworm --> bw_model2["models/particle/bookworm_mace.vmdl_c"]
```

## Prism and Trippy Shared Mechanisms

```mermaid
flowchart TB
    PRISM["prism_recolor_hero_to_addon_tuned"] --> PARTICLE_PRISM["particle spectrum recolor"]
    PRISM --> TEXTURE_PRISM["texture spectrum recolor"]
    PRISM --> MATERIAL_PRISM["material tint spectrum"]
    PRISM --> MODEL_PRISM["vertex COLOR spectrum"]

    PARTICLE_PRISM --> GRADIENTS["gradient stops -> spectral ramps"]
    PARTICLE_PRISM --> COLORS["color/tint arrays -> themed hues"]
    PARTICLE_PRISM --> ANIMATED["optional animation"]
    ANIMATED --> TIMING["texture offset driven by particle age"]
    ANIMATED --> LOOP["loop age-driven color gradients"]
    ANIMATED --> CYCLE["insert runtime color-cycle operator"]

    TEXTURE_PRISM --> NORMAL_TEX["default: recolor_texture_hue"]
    TEXTURE_PRISM --> YAM_STATUS["animated Yamato shadow status:<br/>rainbowize_yamato_shadow_status_texture"]
    TEXTURE_PRISM --> PROJ_TEX["animated projected textures:<br/>rainbowize_projected_texture"]
    TEXTURE_PRISM --> YAM_SHAPE["Yamato shadow shape:<br/>muted cool prism hue"]

    TRIPPY["trippy_ability_vfx_to_addon"] --> RECIPE["recipe_for"]
    TRIPPY --> PAINT_TEX["paint recipe textures with procedural pattern"]
    TRIPPY --> PAINT_PART["reuse particle recolor + prism animation helpers"]
    TRIPPY --> PAINT_MAT["recolor material tints + optional scroll"]
    TRIPPY --> PAINT_MODEL["recolor vertex COLOR models"]
```

## Hero Recipe Catalog

This is the file-level source-of-truth catalog used by the diagrams above. All
texture paths listed here are explicit `texture_entries` in
`HeroRecolorRecipe`. Particle entries are dynamic: every `.vpcf_c` under the
listed prefixes is considered.

| Codename | Particle prefixes | Texture entries | Material entries | Model entries | Preview |
|---|---|---|---|---|---|
| `abrams` | `particles/abilities/abrams/`<br/>`particles/weapon_fx/abrams/` | `materials/particle/abilities/abrams/abrams_leap_ground_impact_hot_symbol_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/abrams_siphon_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/abilities/abrams/abrams_leap_ground_impact_hot_symbol_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `archer` | `particles/abilities/archer/`<br/>`particles/weapon_fx/archer/` | `materials/particle/abilities/archer/archer_charged_shot_gradient_color_v2_psd_51e62704.vtex_c`<br/>`materials/particle/abilities/archer/archer_charged_shot_gradient_color_psd_17c02a47.vtex_c`<br/>`materials/particle/abilities/archer_guided_arrow_explosion_sphere_vmat_g_tcolor_a84e2808.vtex_c`<br/>`materials/models/particle/archer/archer_arrow_illum_vmat_g_tcolor_7d46cca1.vtex_c`<br/>`models/heroes_staging/archer/materials/archer_guided_arrow_color_psd_edd3d0f5.vtex_c`<br/>`models/heroes_staging/archer/bird/materials/bird_color_psd_117d09e0.vtex_c` | none | none | `materials/particle/abilities/archer/archer_charged_shot_gradient_color_v2_psd_51e62704.vtex_c` |
| `astro` | `particles/abilities/astro/`<br/>`particles/weapon_fx/astro/` | none | none | none | none |
| `bebop` | `particles/abilities/bebop/`<br/>`particles/weapon_fx/bebop/` | none | none | none | none |
| `bookworm` | `particles/abilities/bookworm/`<br/>`particles/weapon_fx/bookworm/` | `materials/particle/abilities/bookworm/bookworm_projectile_self_illum_vmat_g_tcolor_7b26a19f.vtex_c`<br/>`materials/particle/projected/bookworm_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/ground/ground_streak_bookworm_psd_5a44028c.vtex_c`<br/>`models/heroes_wip/bookworm/materials/bookworm_ui_effects_color_psd_a29be817.vtex_c`<br/>`models/heroes_wip/bookworm/materials/bookworm_shield_illustrated_color_psd_81f5497b.vtex_c`<br/>`models/heroes_wip/bookworm/materials/bookworm_sword_illustrated_color_psd_4eb22603.vtex_c`<br/>`models/heroes_wip/bookworm/materials/bookworm_stone_illustrated_color_psd_8ed29960.vtex_c`<br/>`models/heroes_wip/bookworm/materials/bookworm_dragon_color_tga_ed3d3b5.vtex_c`<br/>`materials/models/particle/bookworm/neutral_black_dragon_color_psd_b8c8249f.vtex_c` | none | `models/particle/bookworm_horse_knight.vmdl_c`<br/>`models/particle/bookworm_mace.vmdl_c` | `models/heroes_wip/bookworm/materials/bookworm_ui_effects_color_psd_a29be817.vtex_c` |
| `chrono` | `particles/abilities/chrono/`<br/>`particles/weapon_fx/chrono/` | `models/heroes_staging/chrono/materials/chrono_fx_bubble02_color_psd_f57b1ef0.vtex_c`<br/>`models/heroes_staging/chrono/materials/chrono_fx_bubble04_color_psd_ee26af5c.vtex_c` | none | none | `models/heroes_staging/chrono/materials/chrono_fx_bubble04_color_psd_ee26af5c.vtex_c` |
| `digger` | `particles/abilities/digger/`<br/>`particles/weapon_fx/digger/` | `materials/particle/abilities/digger/digger_burrow_channel_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/digger/digger_burrow_explode_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/digger/digger_burrow_spin_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/abilities/digger/digger_burrow_explode_ground_dark_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `doorman` | `particles/abilities/doorman/`<br/>`particles/weapon_fx/doorman/` | `materials/particle/abilities/doorman/doorman_grenade_debuff_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | same as texture |
| `drifter` | `particles/abilities/drifter/`<br/>`particles/weapon_fx/drifter/` | `materials/particle/projected/drifter_claw_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | same as texture |
| `dynamo` | `particles/abilities/dynamo/`<br/>`particles/weapon_fx/dynamo/`<br/>`particles/dynamo/`<br/>`particles/status_fx/status_fx_dynamo` | `materials/particle/projected/dynamo_void_sphere_projected_ground_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/dynamo/dynamo_void_sphere_marker_symbol_psd_73d6401e.vtex_c`<br/>`materials/particle/abilities/dynamo/dynamo_void_sphere_planet_symbol.vtex_c`<br/>`materials/particle/abilities/dynamo/dynamo_void_sphere_sun_halo_symbol.vtex_c`<br/>`materials/particle/abilities/dynamo/dynamo_void_sphere_sun_symbol.vtex_c` | `materials/models/particle/dynamo_void_sphere_cyl.vmat_c`<br/>`materials/models/particle/dynamo_heal_buff_model.vmat_c` | none | `materials/particle/projected/dynamo_void_sphere_projected_ground_vmat_g_tselfillum_670d93d.vtex_c` |
| `familiar` | `particles/abilities/familiar/`<br/>`particles/weapon_fx/familiar/` | `materials/particle/abilities/familiar/familiar_naptime_coneradius_intersection_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/familiar/familiar_pillow_explode_ground_bright_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/familiar/familiar_pillow_explode_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/familiar/familiar_spotlight_ground_edge_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/familiar/familiar_spotlight_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/abilities/familiar/familiar_spotlight_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `fencer` | `particles/abilities/fencer/`<br/>`particles/weapon_fx/fencer/` | `materials/particle/projected/fencer_preview_line_projected_decal_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/fencer_sigil_pentagram_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/fencer/fencer_ult_gradient_color_psd_51322651.vtex_c`<br/>`models/heroes_wip/fencer/materials/fencer_sword_color_tga_52ec8bfe.vtex_c` | none | none | `materials/particle/abilities/fencer/fencer_ult_gradient_color_psd_51322651.vtex_c` |
| `frank` | `particles/abilities/frank/`<br/>`particles/weapon_fx/frank/` | `materials/particle/abilities/frank/frank_painaura_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/frank/frank_revive_marker_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/frank_shock_miss_projected_bright_vmat_g_tselfillum_670d93d.vtex_c` | `materials/particle/abilities/frank/frank_painaura_sphere.vmat_c` | none | `materials/particle/abilities/frank/frank_painaura_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `ghost` | `particles/abilities/ghost/`<br/>`particles/weapon_fx/ghost/` | `models/heroes_staging/ghost/materials/ghost2_clothes_fx_prop_color_psd_b398de35.vtex_c`<br/>`models/heroes_staging/ghost/materials/ghost2_clothes_color_png_fc80b39a.vtex_c` | none | none | `models/heroes_staging/ghost/materials/ghost2_clothes_fx_prop_color_psd_b398de35.vtex_c` |
| `gigawatt` | `particles/abilities/gigawatt/`<br/>`particles/weapon_fx/gigawatt/` | none | none | none | none |
| `haze` | `particles/abilities/haze/`<br/>`particles/weapon_fx/haze/` | `materials/particle/abilities/haze/haze_tracer_self_illum_vmat_g_tcolor_52a5b2da.vtex_c` | none | none | same as texture |
| `hornet` | `particles/abilities/hornet/`<br/>`particles/weapon_fx/hornet/` | none | none | none | none |
| `inferno` | `particles/abilities/inferno/`<br/>`particles/weapon_fx/inferno/` | none | none | none | none |
| `kelvin` | `particles/abilities/kelvin/`<br/>`particles/weapon_fx/kelvin/` | `materials/particle/projected/kelvin_ice_dome_projected_psd_d86c1818.vtex_c`<br/>`materials/particle/projected/kelvin_ice_dome_projected_psd_b5785889.vtex_c`<br/>`models/abilities/materials/ice_dome_color_psd_3a38e562.vtex_c` | none | none | `materials/particle/projected/kelvin_ice_dome_projected_psd_d86c1818.vtex_c` |
| `lash` | `particles/abilities/lash/`<br/>`particles/weapon_fx/lash/` | `materials/particle/cables/lash_cable_material_vmat_g_tcolor_8ca8af3e.vtex_c` | none | none | same as texture |
| `magician` | `particles/abilities/magician/`<br/>`particles/weapon_fx/magician/` | `materials/particle/projected/magician_hex_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/magician/magician_bolt_vmat_g_tcolor_978bc798.vtex_c` | none | none | `materials/particle/projected/magician_hex_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `mcginnis` | `particles/abilities/mcginnis/`<br/>`particles/weapon_fx/mcginnis/` | `materials/particle/abilities/mcginnis/mcginnis_turret_ambient_goo_vmat_g_tcolor_974c5f09.vtex_c`<br/>`materials/particle/abilities/mcginnis/mcginnis_turret_ambient_goo_vmat_g_tsheen_7edd324d.vtex_c` | none | none | `materials/particle/abilities/mcginnis/mcginnis_turret_ambient_goo_vmat_g_tcolor_974c5f09.vtex_c` |
| `mirage` | `particles/abilities/mirage/`<br/>`particles/weapon_fx/mirage/` | none | none | none | none |
| `nano` | `particles/abilities/nano/`<br/>`particles/weapon_fx/nano/` | `materials/particle/abilities/nano/nano_ult_ground_dark_proj_vmat_g_tselfillum_670d93d.vtex_c`<br/>`models/heroes_staging/nano/cat_statue/materials/cat_statue_color_png_8892a790.vtex_c` | none | none | `materials/particle/abilities/nano/nano_ult_ground_dark_proj_vmat_g_tselfillum_670d93d.vtex_c` |
| `necro` | `particles/abilities/necro/`<br/>`particles/weapon_fx/necro/`<br/>`particles/heroes/necro/` | `models/heroes_wip/necro/materials/necro_shambler_color_tga_7b1de566.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_shambler_vmat_g_tnprtransmissivecolor_337e62d.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_jar_of_dread_color_tga_7f34b26.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_jar_glass_color_tga_c6d5a0ec.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_gravestone_color_tga_8a0745c.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_gravestone_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c`<br/>`models/abilities/materials/necro_gravestone_destruction_vmat_g_tnprtransmissivecolor_e8edad5e.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_hand_color_tga_b2300f7f.vtex_c`<br/>`models/heroes_wip/necro/materials/necro_hand_vmat_g_tnprtransmissivecolor_c987b5a.vtex_c` | `models/abilities/materials/necro_pickup_sphere.vmat_c`<br/>`materials/particle/abilities/necro/necro_jar_glass.vmat_c`<br/>`models/abilities/materials/necro_hands.vmat_c`<br/>`models/heroes_wip/necro/materials/necro_flame_effect_hand.vmat_c`<br/>`models/heroes_wip/necro/materials/necro_flame_effect.vmat_c`<br/>`models/heroes_wip/necro/materials/necro_picker_hand_effect.vmat_c`<br/>`models/heroes_wip/necro/materials/necro_picker_effect.vmat_c`<br/>`models/heroes_wip/necro/materials/picker_hand_glow.vmat_c`<br/>`models/heroes_wip/necro/materials/necro_gravestone.vmat_c`<br/>`models/abilities/materials/necro_gravestone_destruction.vmat_c` | none | none |
| `pocket` | `particles/abilities/pocket/`<br/>`particles/weapon_fx/pocket/` | `materials/particle/projected/pocket_satchel_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`models/heroes_staging/synth/materials/pocket_body_color_png_eb808d8a.vtex_c`<br/>`materials/particle/abilities/pocket/pocket_magic_missile_illum_vmat_g_tcolor_754e94bd.vtex_c`<br/>`models/heroes_staging/synth/materials/pocket_suitcase_vmat_g_tcolor_e71e9d59.vtex_c`<br/>`models/abilities/materials/pocket_frog_small_color_png_e2620619.vtex_c`<br/>`models/abilities/materials/synth_deployable_color_psd_a57da819.vtex_c` | none | none | `materials/particle/projected/pocket_satchel_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `priest` | `particles/abilities/priest/`<br/>`particles/weapon_fx/priest/` | `materials/particle/abilities/priest/priest_flashbang_debuff_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/priest/priest_snaptrap_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/priest_snaptrap_projectile_aoe_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/abilities/priest/priest_snaptrap_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `punkgoat` | `particles/abilities/punkgoat/`<br/>`particles/weapon_fx/punkgoat/` | none | none | none | none |
| `shiv` | `particles/abilities/shiv/`<br/>`particles/weapon_fx/shiv/` | none | none | none | none |
| `tengu` | `particles/abilities/tengu/`<br/>`particles/weapon_fx/tengu/` | `materials/particle/cables/ivy_vine_cable_vmat_g_tcolor_9509ed42.vtex_c`<br/>`models/abilities/materials/ivy_entangling_thorns_vmat_g_tcolor_59ac0039.vtex_c` | none | none | `models/abilities/materials/ivy_entangling_thorns_vmat_g_tcolor_59ac0039.vtex_c` |
| `unicorn` | `particles/abilities/unicorn/`<br/>`particles/weapon_fx/unicorn/` | `materials/particle/abilities/unicorn/unicorn_prismatic_shield_ground_warning_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/unicorn_beams_of_light_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/unicorn_flux_rainbow_ground_projected_light_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/unicorn_radiant_flare_ground_advance_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/unicorn_radiant_flare_ground_preview_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/unicorn_radiant_flare_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/projected/unicorn_radiant_flare_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `vampirebat` | `particles/abilities/vampirebat/`<br/>`particles/weapon_fx/vampirebat/` | none | none | none | none |
| `viper` | `particles/abilities/viper/`<br/>`particles/weapon_fx/viper/` | `materials/particle/abilities/viper/viper_petrify_symbol_ground_psd_a643967f.vtex_c` | none | none | same as texture |
| `viscous` | `particles/abilities/viscous/`<br/>`particles/weapon_fx/viscous/`<br/>`particles/abilities/melee/viscous/` | `materials/models/particle/viscous_puddle_telegraph_vmat_g_tcolor_ac749641.vtex_c`<br/>`materials/particle/abilities/viscous/viscous_detail_psd_a2817163.vtex_c`<br/>`materials/particle/abilities/viscous/viscous_detail_psd_3c03ec04.vtex_c`<br/>`materials/particle/abilities/viscous/viscous_detail_psd_4414414e.vtex_c`<br/>`models/heroes_staging/viscous/materials/viscous_punch_preview_vmat_g_tcolor_32414205.vtex_c`<br/>`models/heroes_staging/viscous/materials/viscous_punch_vmat_g_tcolor_afc99362.vtex_c`<br/>`models/heroes_staging/viscous/materials/viscous_fist_dissolve_vmat_g_tcolor_296284fc.vtex_c`<br/>`models/heroes_staging/viscous/materials/viscous_ball_vmat_g_tcolor_2c347bde.vtex_c`<br/>`models/abilities/materials/viscous_cube_color_png_81d0eb6a.vtex_c`<br/>`models/abilities/materials/viscous_cube_color_png_85c8b349.vtex_c`<br/>`models/abilities/materials/viscous_cube_color_png_daff99b9.vtex_c`<br/>`models/abilities/materials/viscous_sphere_color_png_4de0c542.vtex_c`<br/>`models/abilities/materials/viscous_fist_color_psd_ab531623.vtex_c`<br/>`models/abilities/materials/viscous_fist_color_psd_d8e8086a.vtex_c` | `models/abilities/materials/viscous_slime.vmat_c`<br/>`models/abilities/materials/viscous_slime_blobs.vmat_c`<br/>`models/abilities/materials/viscous_cube.vmat_c`<br/>`models/heroes_staging/viscous/materials/viscous_punch.vmat_c`<br/>`models/heroes_staging/viscous/materials/viscous_fist_dissolve.vmat_c`<br/>`models/heroes_staging/viscous/materials/viscous_ball.vmat_c` | none | `materials/models/particle/viscous_puddle_telegraph_vmat_g_tcolor_ac749641.vtex_c` |
| `warden` | `particles/abilities/warden/`<br/>`particles/weapon_fx/warden/` | `materials/models/particle/warden_tech_shield_scanline_color_psd_7e04e0b4.vtex_c` | none | none | same as texture |
| `werewolf` | `particles/abilities/werewolf/`<br/>`particles/weapon_fx/werewolf/` | `materials/particle/abilities/werewolf/werewolf_cripplingslash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/werewolf_transform_bite_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/projected/werewolf_transform_crushing_leap_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` | none | none | `materials/particle/abilities/werewolf/werewolf_cripplingslash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c` |
| `wraith` | `particles/abilities/wraith/`<br/>`particles/weapon_fx/wraith/` | none | none | none | none |
| `yamato` | `particles/abilities/yamato/`<br/>`particles/weapon_fx/yamato/`<br/>`particles/status_fx/status_fx_yamato` | `materials/particle/projected/yamato_blade_dash_ground_projected_vmat_g_tselfillum_670d93d.vtex_c`<br/>`materials/particle/abilities/yamato/yamato_shadow_redemption_complete_status.vtex_c`<br/>`materials/particle/abilities/yamato/yamato_shadow_redemption_nokill_status.vtex_c`<br/>`models/heroes_staging/yamato_v2/materials/yamoto_shadow_shape_color_psd_fe3c64a6.vtex_c` | none | none | `materials/particle/abilities/yamato/yamato_shadow_redemption_complete_status.vtex_c` |

