# Changelog

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
