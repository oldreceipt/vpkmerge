# Changelog

## v0.9.0

Adds spectrum tuning to the `prism` rainbow recolor. The canonical rainbow can now be rotated to a different start hue and scaled in saturation and brightness, so a UI can offer "rotate / desaturate the rainbow" without losing the per-effect spread. Defaults reproduce the v0.8 prism byte for byte. Also wires Yamato into the prism path's chromatic-texture set.

### CLI (`vpkmerge` 0.9)

- `prism` gains `--hue-offset <DEG>` (rotate the whole spectrum's start hue; the per-effect spread is unchanged, just shifted), `--saturation <SCALE>`, and `--brightness <SCALE>` (scale the spectrum, e.g. a pastel rainbow). All three default to the canonical rainbow, so omitting them reproduces the previous output.

### Library (`vpkmerge-core` 0.9)

- New `PrismTuning { hue_offset, saturation, brightness }` and `prism_recolor_hero_to_addon_tuned`, threading the tuning through every spectrum site (particle gradient stops + color fields, textures, materials, models, and the Yamato shadow-band texture). The existing `prism_recolor_hero_to_addon` delegates with `PrismTuning::default()`, so callers that do not expose the knobs (e.g. the GUI Prism tab) are unchanged; default tuning is byte-identical to v0.8.

## v0.8.0

Lands the Deadlock ability-VFX recolor toolkit and the first part-level model edits, plus a large texture-encode speedup. A VFX effect carries its color on up to three axes (particle params, textures, and baked mesh vertex colors); this release can retint all three to one target color, exposes a per-hero "recolor recipe" that drives them together, and adds saturation and brightness control on top of hue. On the geometry side, `model edit` graduates from whole-model reshape to removing and replacing individual mesh parts, and `model export --pose` now poses the WIP heroes whose menu pose ships as a loose motion-matching clip. The recolor and part-edit paths are confirmed in-game.

### CLI (`vpkmerge` 0.8)

- New `texture` subcommand. Recolors a Source 2 `.vtex_c` by setting every pixel's hue (optionally scaling saturation / brightness) to a target, re-encodes the full mip chain in the texture's own format, and packs the result at its entry path so it overrides the base texture with no `.vmat_c` edit. `vpkmerge texture <file|entry> [--from-vpk <vpk>] --hue <DEG> [--saturation <S>] [--brightness <V>] [--preview <PNG>] [--encode OUT] [--encode-vpk OUT_dir.vpk [--vpk-entry PATH]]`. LDR (8-bit) textures only.
- New `recolor-hero` subcommand. Applies a hero's whole recolor recipe (its particle prefix plus any color-bearing textures and vertex-color models) to one target color in a single addon VPK. `--preview-png` renders a fast PNG swatch of the recipe's representative texture (~170ms, no bake) for a live UI preview. Ships the first recipe, Celeste (`unicorn`), a particle-only hero: 228 `.vpcf_c` under `particles/abilities/unicorn/`, no color textures or baked vertex colors.
- New `model recolor` subcommand. Recolors a model's per-vertex `COLOR` attribute (the color some effects bake into the mesh, e.g. Paige's ult horse/knight, that a material tint cannot reach), writing the result without re-encoding meshopt. `vpkmerge model recolor [--list] --vpk <vpk> [--base <vpk>] --hue <DEG> [--saturation <S>] [--brightness <V>] --encode-vpk <OUT_dir.vpk> <ENTRY>...` (multi-model to one addon, mirrors `texture`). The same hue target lands all three recolor axes.
- `model edit` gains part-level edits beyond the v0.6 reshape:
  - `--remove-material <MATERIAL>` removes a mesh part by material (`--list-drawcalls` enumerates them first). Done by zeroing the matching draw calls' index count in a byte-faithful KV3 re-wrap, so the engine loads the edited model with the part gone and no ERROR substitution. Confirmed in-game.
  - `--replace-part <MESH> --from-glb <FILE>` replaces an existing mesh part's geometry in place with a new mesh of any vertex/index count, via block swaps and in-place KV3 scalar edits (no container rebuild).
  - `--export-glb <FILE>` / `--from-glb <FILE>` round-trip a model's mesh through a glb for Blender reshaping: export the editable mesh, move its vertices in Blender (topology preserved), and re-import to write a reshaped addon VPK. `--glb-mesh <NAME>` selects the mesh.
- `model export --pose` now poses WIP heroes (Apollo, Billy, Celeste, Mina, Paige, Rem) whose menu pose ships as a loose `.vnmclip_c` (the newer NM motion-matching format) rather than an embedded clip, so `--require-pose` succeeds for them instead of dropping to a 2D portrait. Also drops Viscous's hidden alt-form Goo Ball mesh from a static posed export (it stayed full-size and swallowed the body).
- `--saturation` / `--brightness` (default 1.0) added to `texture`, `recolor-hero`, and `model recolor`, so a recolor can reach pastels and stop washing out pale source art.

### Library (`vpkmerge-core` 0.8)

- New `recolor` module: `recolor_texture_hue` / `recolor_texture_image` / `recolor_texture_preview_png` (texture), `recolor_model_vertex_colors` (per-vertex COLOR), `inspect_texture`, and the shared `Recolor { hue, saturation, value }` target plus `read_vpk_entry`. One `set_color` drives the texture, particle, and vertex-color paths so a single target lands all three.
- New `hero_recolor` module: `recolor_hero_to_addon` composes a hero's particle + texture + model recolor into one addon VPK from a `HeroRecolorRecipe` (`recipe_for`), `recolor_particle_bytes` retints `.vpcf_c` color params via the byte-faithful KV3 scalar patch, and `recolor_hero_preview_png` renders the live-UI swatch. Reports via `HeroRecolorReport`; `ModelRecolorStats` on the model path.
- New geometry edits on the `model edit` core: `remove_model_material` / `model_draw_call_targets` (draw-call removal), `replace_model_part` (replace a part with a new mesh), plus the glb reshape round-trip. The lossy value-tree KV3 writer is NOT engine-valid for model blocks (it drops value flags and flattens typed-array tags, so the engine substitutes its ERROR model), so every model edit re-wraps the block uncompressed and patches in place.
- WIP-hero posing reads the loose NM clip as a third pose source (after embedded clips and the base-pak donor): `bake_loose_nm_pose` resolves `<dir>/clips/<cand>.vnmclip_c`, reads the referenced `.vnmskel_c`, and bakes the single static card-pose frame onto the model skeleton by bone name.

### morphic (0.4)

- BCn block encoding now runs in parallel across cores. BCn 4x4 blocks carry no cross-block state, so each encode splits into block-row strips compressed with rayon; output is byte-identical to the single-call encoder. The dominant cost in an ability-VFX recolor (Paige's 9 BC7 maps, one 4096x4096) drops from ~27s to ~3.6s on 16 cores (7.4x). Applied to the RGBA8-surface encoders (BC1/BC3/BC7).
- New byte-faithful KV3 editing primitives for model and particle edits. `patch_kv3_resource_scalars(file_bytes, edits)` patches integer scalar fields in a resource's KV3 `DATA` block, located by path (`&[(Vec<kv3::Seg>, i64)]`). Unlike `encode_kv3_resource` (which rebuilds `DATA` from a value tree and so downgrades v5 to v4 and drops value flags + typed array tags, fatal for particles/models), it `rewrap_uncompressed`s the block (preserving v5 framing, flags, and typed tags), applies `kv3::set_scalars`, and rebuilds the container; the lossy re-encode path renders the engine's red error particle. Lower level: `kv3::set_scalars` / `kv3::set_bools` (in-place scalar/bool set), `kv3::neutralize_draw_calls`, and `kv3::rewrap_uncompressed`.
- New vertex-color primitives behind the model recolor: `recolor_vertex_buffer`, `read_vertex_colors`, `OnDiskBuffer::write_colors`, `VertexTarget::has_color`. Both buffer encodings are written without re-encoding meshopt (a re-encode renders garbled in the engine): the uncompressed buffer patches the COLOR bytes in place; the meshopt buffer is decoded, color-edited, and stored uncompressed with `m_bMeshoptCompressed` flipped to false (byte-faithfully via `kv3::set_bools`).
- New Tier 1 model-editing pipeline: a meshopt index encoder (the inverse of the decoder), new-mesh assembly from a glb (`mesh::assemble_to_layout`, skin-weight encode), bone-palette remapping (`skeleton::{invert_remap, localize_joints}`), and `topology::replace_mesh_part`, which splices an assembled mesh into the container via block swaps.
- New NM (motion-matching) format support: `model::nm` (`decode_nm_skeleton` / `decode_nm_pose` / `bake_nm_pose`) reads the static single-frame card pose from a loose `.vnmskel_c` + `.vnmclip_c`, and `pose::bake_pose_named` maps it onto the model skeleton by bone name.
- The GLB writer drops Viscous's hidden-by-default alt-form Goo Ball mesh parts (matched on part name and the `viscous_ball` material token) alongside the existing NPR effect shells, so they stop swallowing the body in a static posed export.

## v0.7.0

Hardens the posed-export path that Grimoire's Locker hero previews depend on. Hero body-model discovery is now deterministic, `--pose` can refuse to emit an unposed model, and Deadlock's comic-outline shells no longer leak into the GLB as a white halo.

### CLI (`vpkmerge` 0.7)

- New `--require-pose` flag on `model export`. With `--pose`, errors out instead of baking a static bind/T-pose when the model carries no menu/idle pose clip (WIP heroes ship the rig but no baked clips). Lets a caller fall back to a 2D portrait rather than show an unposed hero.
- `--hero` body-model discovery is now deterministic: it picks the highest `_vN` model directory consistently instead of relying on VPK enumeration order, so the same hero resolves to the same body model every run.

### Library (`vpkmerge-core` 0.7)

- Pose selection can now signal "no real pose available" so callers can require one (see `--require-pose`); a clipless model no longer silently degrades to the bind pose when a pose was demanded.

### morphic (0.3)

- Drop the non-renderable comic-outline (`*jitter*`) effect shells during GLB export alongside the existing `*_outline` / `*_glow` shells, so they stop collapsing into an opaque white halo in static previews.

## v0.6.0

Adds a static *posed* model export for still previews (a hero card), and stops Deadlock's non-renderable effect shells from leaking into the GLB. `vpkmerge model export --pose` bakes a single animation frame into the mesh and drops the skeleton, skin, and clips, producing a plain posed mesh the size of a static prop. Built for Grimoire's Locker hero previews, where a lightweight still beats shipping a multi-megabyte animated rig.

Also lands the first geometry-editing path: `vpkmerge model edit` reshapes a model's existing vertices (scale and/or translate, topology preserved), re-encodes the meshopt vertex buffers, and packs the result into an addon VPK. Confirmed in-game with a scaled-up hornet body model.

### CLI (`vpkmerge` 0.6)

- New `--pose [CLIP[@FRAME]]` flag on `model export`. Bakes one frame into the vertices and writes a static `.glb` with no skeleton, skin, or animation. Omit the value to try the default menu/roster poses (`ui_hero_pose`, `hero_pose`, `hero_roster_pose`, `hero_roster_ready`, `ui_hero_select`) then generic idles (`idle_loadout`, `primary_stand_idle`) in order, since menu-pose naming differs across heroes; pass a clip name and optional `@frame` to choose explicitly. Mutually exclusive with `--clip` / `--no-anim`.
- New `model edit` subcommand. Reshapes a model's existing geometry by vertex displacement: uniform `--scale` about each part's centroid plus an optional `--translate x,y,z`, with topology preserved. Writes the edited `.vmdl_c` into a standalone addon VPK via `--encode-vpk` (entry path defaults to `--entry`; `--vpk-entry` overrides). `--list` enumerates the editable vertex buffers (mesh part, block index, vertex count) and exits without editing. Reads the mesh from `--vpk` (or `--base` when the skin is texture-only).

### Library (`vpkmerge-core` 0.7)

- `AnimOptions` gains an optional `pose: Option<PoseSelection>`; when set, the export bakes a single posed frame and emits a static mesh, overriding clip / no-anim selection. `PoseSelection { clips, frame }` and `DEFAULT_POSE_CLIPS` are exported; an empty clip list uses the defaults.
- New `edit_model_geometry` plus `GeometryEdit { scale, translate }`, `GeometryEditReport`, and `model_vertex_targets`: decode a `.vmdl_c` from a VPK (or base pak), apply a vertex-displacement edit, re-pack the edited model into an addon VPK, and report the edited parts/buffers/vertices. `model_vertex_targets` lists the editable buffers without editing.
- **Skin mods are posed from the base game's clips.** Skin VPKs ship the mesh + rig but no animation clips, so when the exported model carries none of the requested clips, the same entry is read from the base pak (`--base`) and its clip is mapped onto the skin by bone name (`morphic::model::bake_pose_from`). Same hero, same rig, so no cross-hero retargeting. A hero with no clips anywhere (e.g. an unfinished model) falls back to the bind pose.

### morphic (0.3)

- New `bake_pose(model, clips, frame)`: folds one animation frame into the mesh by linear-blend skinning (each hero's own clip on its own skeleton, so no retargeting), returning a static `Model` with no skeleton, skin weights, or clips. Vertex buffers with no joints pass through unchanged, so a prop posed by its own bone (a held weapon) follows the pose while truly static decor stays put.
- New `bake_pose_from(model, donor, clips, frame)`: bakes `model`'s mesh using a donor model's clips, mapped onto `model`'s skeleton by bone name. For posing a clipless skin with its base-game hero's clip.
- The GLB writer now drops Deadlock's additive glow-effect shells (mesh part `ghost_glow`, `*_glow` materials) alongside the existing inverted-hull `*_outline` shells. As plain glTF geometry both collapse to an opaque "white halo" over the model; their in-game NPR shaders are a renderer-side concern. `*_noglow` materials are kept.
- The vertex decoder now handles 8-influence skinning, unblocking the current Dynamo and Apollo (`fencer`) body models, which previously failed with `unexpected BLENDWEIGHT format`. Their meshes pack up to 8 bones per vertex (an 8-wide `BLENDINDICES` paired with an `R16G16B16A16_UNORM` weight stream of 8 `u8`s). Since the glTF pipeline is fixed at 4 influences, a vertex carrying more keeps its 4 highest-weight bones with the weights renormalized to sum 1; 4-influence meshes are unchanged.
- Texcoord decode now accepts the 1-component `R32_FLOAT` format (V zero-filled), which unblocks the `prof_dynamo` staging model that paired it with the 8-influence skinning above.
- New meshopt vertex encoder (codec v1), the inverse of the existing decoder, plus a `model::edit` module (`vertex_targets`, `read_vertex_positions`, `replace_vertex_positions`). Together they round-trip an edited vertex buffer back into the Source 2 `VBIB` block, preserving topology and every non-position attribute. This is the primitive behind core's `edit_model_geometry`.

## v0.5.0

Adds a Source 2 model exporter. `vpkmerge model export` turns a Deadlock hero `.vmdl_c` (from the base pak or a skin VPK) into a textured, skinned, animated `.glb`, decoded entirely in pure Rust (a faithful port of ValveResourceFormat, no .NET or C runtime). The exported model carries the hero's own animation clips on its own skeleton, so a viewer can play its idle without any cross-hero retargeting. Built for Grimoire's hero/skin preview.

### CLI (`vpkmerge` 0.5)

- New `model` subcommand. `model <vpk>` inspects the compiled models in a VPK (block structure, mesh-part count, embedded geometry, skeleton/physics presence). `model export` writes a `.glb`: choose the source with `--entry <path.vmdl_c>` or `--hero <codename>` (auto-discovers the body model), resolve materials/textures across `--vpk` then `--base <pak01_dir.vpk>`, and write to `--out`. Animation clips are emitted by default; `--clip <name>` (repeatable) exports only the named clip(s) and `--no-anim` drops them. The positional merge invocation and the `split` / `portrait` / `soundevents` subcommands are unchanged.

### Library (`vpkmerge-core` 0.6)

- `export_model`, `export_hero_model`, and `inspect_models` plus `AnimOptions`: open a VPK (and optional base pak), decode a `.vmdl_c` via `morphic`, and write a textured, animated `.glb`, resolving referenced materials/textures across both packages (skin first, base second).

### morphic (0.2)

- New `model` module: a pure-Rust Source 2 `.vmdl_c` decoder and glTF writer. Decodes the skeleton, LOD0 meshes (meshoptimizer vertex/index codecs), per-vertex skin weights, materials (`.vmat_c`) with their `.vtex_c` textures, and the model's own animation clips (`ANIM`/`ASEQ`/`AGRP`: the compressed segment decoders, the 6-byte packed quaternion, half-float vectors), then emits a binary `.glb` (geometry + skin + PBR materials + animation samplers). The KV3 reader gained ZSTD (via `ruzstd`) and the binary-blob section, which the model `ANIM` block uses.

## v0.4.0

Closes the gap between editing a soundevents file and shipping it: the `soundevents` subcommand can now pack its re-encoded output straight into a standalone addon VPK, so an edited (or generated) loose file can finally enter the merge pipeline. This unblocks Grimoire's per-ability volume/pitch path, where a hero's `.vsndevts_c` is decoded, `--set` on the relevant events, and the result shipped in a consolidated addon VPK at the same entry path.

### CLI (`vpkmerge` 0.4)

- New `--encode-vpk OUT_dir.vpk` flag on `soundevents`. After applying `--swap-vsnd` / `--set`, re-encode and pack the file into a standalone single-archive VPK at its entry path, ready to merge. The entry path defaults to INPUT in `--from-vpk` mode; `--vpk-entry PATH` overrides it (and is required for a loose-file input). Combinable with `--encode`; the loose `--encode` and JSON-to-stdout behaviors are unchanged.

### Library (`vpkmerge-core` 0.5)

- New `pack(files, output)`: write in-memory `(entry_path, bytes)` files into a standalone single-archive `_dir.vpk`. The general primitive for getting loose or generated files into a VPK so they can enter `merge`. Creates missing parent directories; produces a chunk-free, engine-loadable addon VPK.

## v0.3.0

vpkmerge grows past pure merging into a small Deadlock asset toolkit: two new CLI subcommands (hero portrait extraction and soundevents editing), a GUI Browse tab that walks a VPK's file tree with live texture previews, and a much-expanded `morphic` that now decodes HDR textures, selects mips/cubemap faces, re-encodes, and reads/writes binary KeyValues3.

### CLI (`vpkmerge` 0.3)

- New `portrait` subcommand. Extract and decode hero card/portrait art from a VPK to PNG. Flags: `--out DIR`, `--hero CODENAME` (omit for every hero), `--manifest FILE` (otherwise the JSON manifest prints to stdout).
- New `soundevents` subcommand. Decode a `.vsndevts_c` file to JSON, edit it, and re-emit a loadable file. Reads a file on disk or, with `--from-vpk VPK`, an entry path inside a VPK. Edits: `--swap-vsnd OLD=NEW` (rewrite a clip path everywhere), `--set EVENT/FIELD=NUMBER` (set a numeric field like volume/pitch). `--encode OUT` writes an uncompressed v4 file the engine can load; without it the decoded tree prints as JSON.
- The positional merge invocation and the `split` subcommand are unchanged.

### Library (`vpkmerge-core` 0.4)

- `extract_portraits(vpk, hero, out_dir)` plus `PortraitInfo` and `PortraitVariant`: locate hero portrait textures in a VPK and decode them to PNG (via `morphic`), reporting per-texture format/size and skip reasons.
- `SoundEvents` (load `from_file` / `from_vpk` / `from_bytes`, `to_json`, `swap_vsnd`, `set_event_field`, `encode`, `original_len`) plus `EventSummary`: the soundevents-aware layer over `morphic`'s KV3 codec, built for a per-ability sound picker (control volume/pitch/clip choice, not just swap audio).

### GUI (0.3)

- **Browse tab.** New top-level tab alongside Merge. Auto-loads the local Deadlock `pak01_dir.vpk`, walks the VPK file tree with a filter, and renders a live preview of any selected `.vtex_c` via `morphic`.
- **Collision default flipped to top-of-list-wins.** The highest-priority (top) mod now overrides by default, matching the visible priority order. Still exposes Bottom wins and Refuse.
- **HDR previews tone-mapped** instead of erroring out, so BC6H / float textures show a sensible thumbnail.
- Long entry paths in the Browse tab scroll horizontally instead of overflowing.

### `morphic` 0.1.0

- **BC6H decode** (HDR, `Rgba16F` output) with an HDR golden path: LDR formats diff against a PNG, HDR formats against a raw `.f32` sibling with per-channel tolerance.
- **Encoders + splicing.** `encode_image` (BCn, BC6H, RGBA8, inline PNG passthrough) plus splice entry points: `replace_face0_mip0` / `replace_face_mip` (single mip, rest byte-exact) and `replace_mip_chain` / `replace_face_mip_chain` (regenerate the full mip pyramid from a new mip 0).
- **Mip + cubemap face selection.** `DecodeOptions { mip, slice, face }` decodes any mip level or cubemap face.
- **Binary KeyValues3 codec** (`morphic::kv3`): reads v1..=5 (including v5 two-buffer / LZ4), writes v4 uncompressed. `decode_kv3_resource` / `encode_kv3_resource` wrap the resource envelope, preserving the format GUID and `RED2` block on re-encode. This is what powers `SoundEvents`.
- Format coverage is now roughly 86% of Deadlock's `.vtex_c` corpus by count. RGBA16161616F, inline WebP, 3D depth slices, and texture arrays remain pending (non-blocking in the golden harness).

### Packaging / release

- **AUR packages** under `packaging/aur/`: `vpkmerge-bin` (GUI from the release `.deb`), `vpkmerge-cli-bin` (raw CLI binary), and `vpkmerge-git` (both, built from HEAD).
- Portable Windows `.exe` added to the release artifacts (no install; needs WebView2).
- Release notes now lead with a per-platform Download table for both the desktop app and the CLI.

## v0.2.0

The first release with both directions of the operation: combine many VPKs into one, or split one into many. The GUI gets a substantial visual overhaul and the conflict resolver gains texture-aware previews.

### Library (`vpkmerge-core` 0.3)

- New `split` API symmetric to `merge`: route entries from one input VPK into N outputs by path predicate, with optional residual bucket and three overlap policies (`FirstMatch`, `AllMatches`, `Error`).
- New types: `SplitOutput`, `PathPredicate::AnyPrefix`, `SplitOptions`, `SplitReport`, `SplitOutputReport`.

### CLI (`vpkmerge` 0.2)

- New `split` subcommand. Takes a JSON plan file describing outputs and prefix predicates; flags: `--strict`, `--all-matches`, `--residual`, `--verbose`. See [`docs/splicing.md`](./docs/splicing.md).
- Existing positional merge invocation continues to work unchanged.

### GUI (0.2)

- **Settings**: gear in the title bar opens a Settings panel.
  - Theme: Light / Dark / System (follows OS in real time).
  - Doodles: four backgrounds (arcane, celestial, botanical, nautical) in light and dark variants.
  - Candlelight: optional warm corner glow with a slow flicker and sway.
  - Diagnostics: export the current session log to a text file.
- **Texture preview** in the conflict resolver: `.vtex_c` collisions now show thumbnails of each candidate's texture so you can pick the winner by sight. Powered by the new `morphic` decoder.
- **Layout**: the bottom bar is gone. Merge is now a paper-card section at the end of the form, with a live summary, the conflicts pill, and a prominent Merge button (wax-seal shadow treatment so it doesn't blend with the radio buttons above).
- **Merged modal**: animated checkmark on the success header.
- **Merge with no output set**: clicking Merge now opens the save picker instead of erroring.

### New crate (`morphic` 0.0.1)

Pure-Rust Source 2 `.vtex_c` decoder. Format support: BC7, DXT1, DXT5, ATI1N, ATI2N, RGBA8888, and the inline PNG path. A committed golden corpus diffs each decode against a ValveResourceFormat-generated reference. Not on crates.io yet.

### Repo

- `morphic-oracle` C# dev harness under `tools/morphic-oracle/` regenerates the golden corpus. CI never runs `dotnet`.
- `Justfile` orchestrates the daily loop (`just`, `just ci`, `just survey`, `just fixture`).
- Cross-OS release workflow at `.github/workflows/release.yml` ships GUI bundles and CLI binaries for Linux, macOS (x86_64 + aarch64), and Windows on every `v*.*.*` tag.

## v0.1.x

Earlier releases. See `git log` for details.
