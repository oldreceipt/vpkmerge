# Handoff: promote `skin_pack` into a `vpkmerge texture edit` subcommand

## Context

The texture-reskin round-trip is **proven** end to end (edit a hero albedo PNG ->
re-encode + splice into the original `.vtex_c` -> pack an addon VPK -> renders in
Deadlock). It currently lives as a throwaway example, `morphic/examples/skin_pack.rs`.
This handoff is to graduate it into a real, tested CLI subcommand.

Read [findings-deadlock-skin-textures.md](./findings-deadlock-skin-textures.md) first
for the file-path, material, and round-trip details that motivate the design.

## What already exists (reuse, do not rebuild)

All the hard parts are library code:

- `morphic::inspect(&bytes) -> TextureInfo` (format, width, height, mip_count)
- `morphic::decode(&bytes) -> Image` (to recover the original alpha channel)
- `morphic::replace_mip_chain(orig_bytes, &new_mip0) -> Vec<u8>` re-encodes mip 0 in the
  texture's native format and rebuilds the full mip pyramid, splicing into the original
  resource envelope. (Internally uses `morphic::encode_image`, which supports DXT1/DXT5/
  ATI1N/ATI2N/BC7/BC6H/RGBA8/BGRA8/PNG.)
- `vpkmerge_core::pack(&[(&str, &[u8])], out)` writes loose files into a VPK at the given
  internal paths (the same primitive `soundevents --encode-vpk` uses).

So the subcommand is mostly glue + arg parsing + tests.

## Model it on the existing `soundevents` flow

`vpkmerge soundevents <file> [--from-vpk <vpk>] [--encode-vpk OUT_dir.vpk [--vpk-entry PATH]]`
already does "load from a VPK, edit, re-encode, pack a standalone addon VPK." Mirror its
shape and its arg conventions (`--from-vpk`, `--encode-vpk`, `--vpk-entry`).

## Proposed CLI

```
vpkmerge texture edit <PNG> \
    --from-vpk <pak01_dir.vpk> \
    --entry <models/.../foo_color_png_HASH.vtex_c> \
    --encode-vpk <OUT_dir.vpk> \
    [--vpk-entry <override path>] \
    [--no-preserve-alpha]
```

- `<PNG>`: the edited albedo. Must match the source texture's width/height.
- `--from-vpk` + `--entry`: the original compiled `.vtex_c` to splice into (provides the
  format, mip count, and original alpha).
- `--encode-vpk`: output addon VPK. By default packs the new `.vtex_c` at `--entry`
  (so it overrides pak01); `--vpk-entry` overrides that path.
- `--no-preserve-alpha`: opt out of copying the original alpha channel (default is to
  preserve it; Source albedo alpha can carry a mask, see findings doc).

Consider also a loose-file mode (`--from-file <orig.vtex_c>` instead of `--from-vpk`/
`--entry`) for symmetry with `soundevents`, but VPK mode is the primary use.

## Reference implementation

`morphic/examples/skin_pack.rs` already does the full flow; lift its body:

1. Open `--from-vpk`, read `--entry` bytes; `inspect` to get format/size.
2. `decode` the original, extract its alpha (every 4th byte of the RGBA8 buffer).
3. Load the PNG (`image` crate) to RGBA8; assert dimensions == texture dimensions.
4. Unless `--no-preserve-alpha`, copy the original alpha into the PNG buffer.
5. Build `morphic::Image { width, height, data: ImageData::Rgba8(buf) }`.
6. `morphic::replace_mip_chain(&orig, &img)` -> new `.vtex_c` bytes.
7. `vpkmerge_core::pack(&[(entry_or_override, &new_vtex)], encode_vpk_out)`.

Where this lives: the subcommand belongs in `vpkmerge-cli` (it needs both `morphic` and
`vpkmerge_core::pack`), alongside `run_soundevents` / `run_model` in
`vpkmerge-cli/src/main.rs`.

## Edge cases / correctness

- **Dimension mismatch**: hard error with a clear message (PNG must be exactly the
  texture's mip-0 size). Do not silently resize.
- **HDR textures** (`Rgba16F`, e.g. BC6H): the PNG path assumes 8-bit. Either reject HDR
  source textures with a clear error or handle them separately. Most hero albedos are
  BC7/DXT, so 8-bit is the common path.
- **Unsupported encode format**: `encode_image` returns `EncodeError::Unimplemented`;
  surface it cleanly.
- **Alpha**: default to preserving the original alpha. The galaxy test confirmed the
  dress alpha was empty, but other textures may use it as a mask.
- **Codename / cross-dir paths**: the `--entry` is the literal `.vtex_c` path from the
  pak (resolve it via `mat_dump` on the hero's `.vmat`, or `list_paths`). Materials can
  live cross-directory from the model (see findings doc).

## Tests

- **Golden round-trip** (no .NET, pure Rust, CI-safe): take a committed fixture
  `.vtex_c` (the `morphic/fixtures/` corpus has per-format samples incl. `bc7/`), decode
  it to a PNG, edit a pixel, run the encode+splice, decode the result, and assert the
  edited pixel changed and the rest round-trips within BC7 tolerance. Assert the output
  `.vtex_c` `inspect`s to the same format/size/mip_count as the input.
- **Pack test**: assert the produced VPK contains exactly one entry at the expected path
  and that `valve_pak::open` can read it back (mirror the existing `pack` test in
  `vpkmerge-core/src/lib.rs`).
- Note in the doc/test that byte-exact VPK output is not reproducible across runs
  (`from_directory` walks the FS in OS order; known limitation in the project README).

## Out of scope (note for later)

- **Recolor logic** (zone-aware HSV remap, galaxy generation) was done ad hoc in Blender
  Python. The CLI subcommand only handles the encode/pack half (PNG -> in-game). If we
  later want scripted recolors, that is a separate feature; for now the artist produces
  the PNG (Blender, GIMP, etc.) and this subcommand ships it.
- **GUI integration**: eventually the Tauri GUI / Grimoire client should expose this as
  a button. The CLI subcommand is the reusable core for that.

## Done when

`vpkmerge texture edit <png> --from-vpk <vpk> --entry <path> --encode-vpk out_dir.vpk`
produces an addon VPK that loads in Deadlock and shows the edited texture, with the
golden round-trip and pack tests green under `cargo test --workspace`, and `clippy
--all-targets -D warnings` + `fmt --check` clean. Then delete the throwaway examples
(or keep only the inspection ones) once the subcommand covers the flow.
