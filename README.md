# vpkmerge

[![CI](https://github.com/Slush97/vpkmerge/actions/workflows/ci.yml/badge.svg)](https://github.com/Slush97/vpkmerge/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Slush97/vpkmerge?logo=github&color=blue)](https://github.com/Slush97/vpkmerge/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Slush97/vpkmerge/total?logo=github)](https://github.com/Slush97/vpkmerge/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)](https://github.com/Slush97/vpkmerge/releases/latest)

Combine multiple Valve Pak (`.vpk`) files into one, or split one back into many. Plus a small toolkit for the assets inside: decode Source 2 textures, extract hero portraits, read/edit soundevents, and export compiled models to glTF.

Built for **Deadlock** modding: pre-merging several mods into one VPK consolidates them into a single addon slot, so a mod manager can resolve load order and overrides up front instead of dropping a pile of loose VPKs into the game. Splitting is the inverse operation for mod managers that want per-feature granularity (e.g. one ability slot at a time).

What it does:

- **Merge** many VPKs into one, with collision policies and per-path overrides.
- **Split** one VPK into many by path predicate (the inverse of merge).
- **Portrait**: extract and decode hero card/portrait art from a VPK to PNG.
- **Soundevents**: decode a Deadlock `.vsndevts_c` file to JSON, edit clip paths and params (volume, pitch), and re-emit a file the engine can load.
- **Model**: export a hero `.vmdl_c` to a textured, skinned, animated binary glTF (`.glb`), decoded entirely in pure Rust (no .NET or C runtime).
- **Desktop GUI**: drag-and-drop merging with a visual conflict resolver, plus a Browse tab to walk a VPK's file tree and preview textures.
- **`morphic`**: a pure-Rust Source 2 decoder underpinning the rest: `.vtex_c` textures (decode/encode), binary KeyValues3, and `.vmdl_c` -> `.glb` model export (no .NET runtime required).

## Download

Prebuilt downloads live on the **[Releases page](https://github.com/Slush97/vpkmerge/releases/latest)**. Two flavors:

- **Desktop app** (the GUI most people want): drag-and-drop file picker, conflict resolver with texture previews, themes, dark/light mode.
- **Command-line tool** (`vpkmerge`): for scripts, mod-manager integrations, or anyone who lives in a terminal.

Grab the right file for your system:

### Desktop app

| Your computer                | File to download                                                       |
| ---------------------------- | ---------------------------------------------------------------------- |
| **Windows**                  | `vpkmerge_<version>_x64-setup.exe` (recommended) or the `.msi`         |
| **macOS** (Apple Silicon, M1/M2/M3/M4) | `vpkmerge_<version>_aarch64.dmg`                              |
| **macOS** (Intel)            | Use the Apple Silicon `.dmg` under Rosetta 2, or build from source     |
| **Linux** (Debian/Ubuntu)    | `vpkmerge_<version>_amd64.deb`                                         |
| **Linux** (Fedora/RHEL)      | `vpkmerge-<version>-1.x86_64.rpm`                                      |
| **Linux** (anything else)    | `vpkmerge_<version>_amd64.AppImage` (no install, just `chmod +x` and run) |

On Windows, double-click the installer. On macOS, open the `.dmg` and drag the app into Applications (you may need to right-click → Open the first time because the app isn't notarized). On Linux, install the `.deb`/`.rpm` with your usual package tool, or make the AppImage executable and double-click it.

### Command-line tool

| Your computer                          | File to download                |
| -------------------------------------- | ------------------------------- |
| **Linux** (x86_64)                     | `vpkmerge-linux-x86_64`         |
| **macOS** (Apple Silicon)              | `vpkmerge-macos-aarch64`        |
| **Windows** (x86_64)                   | `vpkmerge-windows-x86_64.exe`   |

On Linux/macOS, run `chmod +x vpkmerge-*` once after downloading, then call it from a terminal. On Windows it's already executable.

## Layout

This repo is a Cargo workspace with four crates:

- [`vpkmerge-core/`](./vpkmerge-core) (v0.6): pure Rust library with the merge and split engines plus the portrait-extraction, soundevents, and model-export layers. No UI dependencies. Reusable from any Rust project.
- [`vpkmerge-cli/`](./vpkmerge-cli) (v0.5): the `vpkmerge` command-line binary (`merge`, `split`, `portrait`, `model`, `soundevents`).
- [`gui/src-tauri/`](./gui/src-tauri) (v0.5): Tauri v2 desktop app with a visual conflict resolver, a Browse tab for walking a VPK's file tree, a themeable paper UI, and texture preview for Source 2 `.vtex_c` entries (Vue 3 + Tailwind frontend in [`gui/src/`](./gui/src)).
- [`morphic/`](./morphic) (v0.2): pure-Rust Source 2 decoder. Decodes `.vtex_c` textures in LDR and HDR (BC6H), selecting mips/cubemap faces and re-encoding; reads/writes binary KeyValues3 (`.vsndevts_c`); and decodes `.vmdl_c` models (skeleton, skinned LOD0 meshes, materials, animation clips) to binary glTF. Powers the GUI texture previews, the soundevents layer, and the model exporter. See [`morphic/README.md`](./morphic/README.md).

## CLI

### Build

```bash
git clone https://github.com/Slush97/vpkmerge
cd vpkmerge
cargo build --release -p vpkmerge-cli
```

Binary lands at `target/release/vpkmerge`.

### Merge

```bash
vpkmerge <output_dir.vpk> <input1_dir.vpk> <input2_dir.vpk> [more.vpk...] [options]
```

| Flag | Description |
|------|-------------|
| `--strict` | Error out on any path collision instead of letting later inputs win |
| `--verbose`, `-v` | Print each path that gets overridden by a later input |
| `--help`, `-h` | Show usage |
| `--version`, `-V` | Show version |

**Collision policy.** By default, later inputs win: if two VPKs contain the same path, the version from the VPK passed later on the command line is kept. Use `--strict` to refuse to merge when any path appears in more than one input.

**Chunked inputs.** For VPKs split across `*_dir.vpk` + `*_000.vpk`, `*_001.vpk`, ... pass only the `_dir.vpk` file. Chunk files are read automatically when they sit alongside it.

### Split

```bash
vpkmerge split <input_dir.vpk> --plan plan.json [--residual leftovers.vpk] [--strict | --all-matches] [-v]
```

Routes entries from one input VPK into N outputs by path predicate. The plan is a small JSON file:

```json
{
  "outputs": [
    { "path": "a2_only.vpk", "prefixes": ["sounds/abilities/abrams/a2_"] },
    { "path": "a4_only.vpk", "prefixes": ["sounds/abilities/abrams/a4_"] }
  ],
  "residual": "leftovers.vpk"
}
```

By default each path goes to the FIRST matching output. `--all-matches` routes each path to EVERY matching output; `--strict` refuses to split if any path matches more than one output. Entries that no predicate claims either go to `residual` if set, or are dropped silently (the count appears in the report either way).

See `vpkmerge split --help` for the full option list, and [`docs/splicing.md`](./docs/splicing.md) for the design spec and the motivating use case.

### Portrait

```bash
vpkmerge portrait <input_dir.vpk> --out <dir> [--hero <codename>] [--manifest <file.json>]
```

Find hero card/portrait textures in a VPK and decode them to PNG. Without `--hero` it extracts every hero in the VPK; pass a codename (e.g. `--hero hornet`) to limit it to one. A JSON manifest describing each texture (hero, variant, dimensions, format, output path, and a skip reason for anything not decodable) prints to stdout, or to `--manifest <file>` if given.

```bash
vpkmerge portrait pak01_dir.vpk --out ./portraits --hero hornet
```

### Model

```bash
# Inspect the compiled models in a VPK.
vpkmerge model <input_dir.vpk>

# Export one model to a textured, skinned, animated .glb.
vpkmerge model export --vpk <vpk> (--entry <path.vmdl_c> | --hero <codename>) \
  [--base <pak01_dir.vpk>] [--clip <name>]... [--no-anim] --out <file.glb>
```

`model <vpk>` (no subcommand) lists each `.vmdl_c` in the VPK with its block structure, mesh-part count, and whether it carries embedded geometry / skeleton / physics.

`model export` decodes a single `.vmdl_c` to a binary glTF, in pure Rust (no .NET or C runtime). It writes the skeleton, the skinned LOD0 meshes, PBR materials with their `.vtex_c` textures, and the model's own animation clips on its own skeleton (so a viewer can play its idle with no cross-hero retargeting).

| Flag | Description |
|------|-------------|
| `--vpk <VPK>` | VPK containing the `.vmdl_c` (a skin VPK, or the base pak itself) |
| `--entry <PATH>` | VPK-internal model path, e.g. `models/heroes_staging/hornet_v3/hornet.vmdl_c`. Mutually exclusive with `--hero` |
| `--hero <CODENAME>` | Hero codename (e.g. `hornet`) whose body model is auto-discovered. Mutually exclusive with `--entry` |
| `--base <VPK>` | Base `pak01_dir.vpk` that referenced materials/textures resolve against when the skin VPK does not ship them (skin first, base second) |
| `--clip <NAME>` | Export only the named clip(s). Repeatable. Omit to export every clip; `--no-anim` to drop them all |
| `--no-anim` | Export the static mesh + skeleton only (no animation) |
| `--out <FILE>` | Output `.glb` path |

```bash
# Export a hero's body model from the base pak, all clips, to a .glb.
vpkmerge model export \
  --vpk ~/.steam/steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk \
  --hero hornet \
  --out hornet.glb
```

### Soundevents

```bash
vpkmerge soundevents <file.vsndevts_c | entry-path> \
  [--from-vpk <vpk>] \
  [--swap-vsnd OLD=NEW]... \
  [--set EVENT/FIELD=NUMBER]... \
  [--encode <out.vsndevts_c>]
```

Decode a Deadlock soundevents file (binary KeyValues3) to JSON, optionally edit it, and re-emit a file the engine can load. Read a file on disk, or read an entry from inside a VPK with `--from-vpk`.

| Flag | Description |
|------|-------------|
| `--from-vpk <vpk>` | Read the positional argument as an entry path inside this VPK instead of a file on disk |
| `--swap-vsnd OLD=NEW` | Rewrite a clip path everywhere in the tree (repeatable) |
| `--set EVENT/FIELD=NUMBER` | Set a numeric field on one event, e.g. `--set "Seven.Wpn.Fire/volume=0.25"` (repeatable) |
| `--encode <out>` | After edits, write an uncompressed v4 file (loadable by the engine). Without it, the decoded tree prints as JSON |

```bash
# Read from the game VPK, halve an ability's volume, write a loadable file.
vpkmerge soundevents soundevents/hero/gigawatt.vsndevts_c \
  --from-vpk pak01_dir.vpk \
  --set "Seven.Wpn.Fire/volume=0.5" \
  --encode gigawatt.vsndevts_c
```

See [`docs/spike-vsndevts-kv3.md`](./docs/spike-vsndevts-kv3.md) for the format writeup.

### Example (merge)

```bash
vpkmerge combined_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak04_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak05_dir.vpk \
  --verbose
```

Drop the resulting `combined_dir.vpk` into `citadel/addons/` to mount it as a single mod slot.

## GUI

Tauri v2 desktop app that wraps the same engine. Features:

- Drag-and-drop file input, reorderable mod priority, per-conflict overrides (top of the list wins by default)
- Visual conflict resolver with texture thumbnails for `.vtex_c` entries (powered by `morphic`), HDR textures tone-mapped so they preview instead of erroring
- **Browse tab**: auto-loads your local Deadlock `pak01_dir.vpk`, walks the VPK file tree with a filter, and previews any selected texture
- Custom paper-stationery theme: light / dark / system, four doodle backgrounds (arcane, celestial, botanical, nautical), and an optional candle-light vignette
- Settings persist across launches via `localStorage`

```bash
cd gui
pnpm install
pnpm tauri dev    # development
pnpm tauri build  # release bundle
```

Requires Rust, Node 18+, pnpm, and the [Linux system dependencies Tauri lists for your distro](https://v2.tauri.app/start/prerequisites/#linux).

## Library

To use the merge / split engines from another Rust project:

```toml
[dependencies]
vpkmerge-core = "0.6"
```

```rust
use vpkmerge_core::{merge, MergeOptions, split, SplitOutput, PathPredicate, SplitOptions};

// Merge: many VPKs to one.
merge(
    &["mod_a_dir.vpk", "mod_b_dir.vpk"],
    "combined_dir.vpk",
    &MergeOptions::default(),
)?;

// Split: one VPK to many.
let outputs = vec![
    SplitOutput {
        path: "a2_only.vpk".into(),
        predicate: PathPredicate::AnyPrefix(vec!["sounds/abilities/abrams/a2_".into()]),
    },
];
split("source_dir.vpk", &outputs, &SplitOptions::default())?;
```

Also exposes `inspect(path)` (list a VPK's contents), `detect_conflicts(inputs)` (preview path collisions without writing anything), and `pack(files, output)` (write loose/generated files into a standalone VPK), plus the higher-level asset layers: `extract_portraits(vpk, hero, out_dir)` for decoding hero art to PNG, `SoundEvents` (`from_file` / `from_vpk`, `to_json`, `swap_vsnd`, `set_event_field`, `encode`) for reading and editing `.vsndevts_c` files, and `export_model` / `export_hero_model` / `inspect_models` (with `AnimOptions`) for turning a `.vmdl_c` into a textured, animated `.glb`.

## License

MIT. See [`LICENSE`](./LICENSE). Texture-decoding algorithms in `morphic/` are adapted from [ValveResourceFormat](https://github.com/ValveResourceFormat/ValveResourceFormat) (MIT); per-file attribution lives in [`morphic/LICENSE-THIRD-PARTY`](./morphic/LICENSE-THIRD-PARTY).
