# vpkmerge

Combine multiple Valve Pak (`.vpk`) files into one, or split one back into many.

Built for **Deadlock** modding: the game caps mounted mod VPKs at roughly 100, so pre-merging several mods into one VPK lets players run more mods than the engine would otherwise allow. Splitting is the inverse operation for mod managers that want per-feature granularity (e.g. one ability slot at a time).

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

- [`vpkmerge-core/`](./vpkmerge-core) (v0.3) — pure Rust library with the merge and split engines. No UI dependencies. Reusable from any Rust project.
- [`vpkmerge-cli/`](./vpkmerge-cli) (v0.2) — the `vpkmerge` command-line binary.
- [`gui/src-tauri/`](./gui/src-tauri) (v0.2) — Tauri v2 desktop app with a visual conflict resolver, themeable paper UI, and texture preview for Source 2 `.vtex_c` collisions (Vue 3 + Tailwind frontend in [`gui/src/`](./gui/src)).
- [`morphic/`](./morphic) (v0.0) — pure-Rust Source 2 `.vtex_c` texture decoder. Used by the GUI to render thumbnails of conflicting textures so you can decide which mod wins by sight, not by filename. See [`morphic/README.md`](./morphic/README.md).

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

- Drag-and-drop file input, reorderable mod priority, per-conflict overrides
- Visual conflict resolver with texture thumbnails for `.vtex_c` entries (powered by `morphic`)
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
vpkmerge-core = "0.3"
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

Also exposes `inspect(path)` (list a VPK's contents) and `detect_conflicts(inputs)` (preview path collisions without writing anything).

## License

MIT. See [`LICENSE`](./LICENSE). Texture-decoding algorithms in `morphic/` are adapted from [ValveResourceFormat](https://github.com/ValveResourceFormat/ValveResourceFormat) (MIT); per-file attribution lives in [`morphic/LICENSE-THIRD-PARTY`](./morphic/LICENSE-THIRD-PARTY).
