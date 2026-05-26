# Changelog

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
