# vpkmerge

Combine multiple Valve Pak (`.vpk`) files into one.

Built for **Deadlock** modding: the game caps mounted mod VPKs at roughly 100, so pre-merging several mods into one VPK lets players run more mods than the engine would otherwise allow.

## Layout

This repo is a Cargo workspace with three crates:

- [`vpkmerge-core/`](./vpkmerge-core) — pure Rust library with the merge engine. No UI dependencies. Reusable from any Rust project.
- [`vpkmerge-cli/`](./vpkmerge-cli) — the `vpkmerge` command-line binary.
- [`gui/src-tauri/`](./gui/src-tauri) — Tauri v2 desktop app with a visual conflict resolver (Vue 3 + Tailwind frontend in [`gui/src/`](./gui/src)).

## CLI

### Build

```bash
git clone https://github.com/Slush97/vpkmerge
cd vpkmerge
cargo build --release -p vpkmerge-cli
```

Binary lands at `target/release/vpkmerge`.

### Usage

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

### Example

```bash
vpkmerge combined_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak04_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak05_dir.vpk \
  --verbose
```

Drop the resulting `combined_dir.vpk` into `citadel/addons/` to mount it as a single mod slot.

## GUI

A Tauri v2 desktop app that wraps the same engine with a visual conflict resolver, drag-and-drop file input, and reorderable mod priority.

```bash
cd gui
pnpm install
pnpm tauri dev
```

Requires Rust, Node 18+, pnpm, and the [Linux system dependencies Tauri lists for your distro](https://v2.tauri.app/start/prerequisites/#linux).

## Library

To use the merge engine from another Rust project:

```toml
[dependencies]
vpkmerge-core = { git = "https://github.com/Slush97/vpkmerge" }
```

```rust
use vpkmerge_core::{merge, MergeOptions};

merge(
    &["mod_a_dir.vpk", "mod_b_dir.vpk"],
    "combined_dir.vpk",
    &MergeOptions::default(),
)?;
```

Also exposes `inspect(path)` (list a VPK's contents) and `detect_conflicts(inputs)` (preview path collisions without writing anything).
