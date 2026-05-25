# morphic-oracle

Dev-time golden-output generator for the [morphic](../../morphic) Rust
decoder. Wraps [ValveResourceFormat](https://github.com/ValveResourceFormat/ValveResourceFormat)
to produce `.png` + `.meta.json` siblings for each `.vtex_c` fixture so the
pure-Rust test harness has something to diff against.

## Prerequisites

- .NET 10 SDK (`10.0.300` pinned in `global.json`). One-liner local install:
  ```bash
  curl -fsSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel 10.0
  export PATH="$HOME/.dotnet:$PATH"
  ```

Lockfile (`packages.lock.json`) is committed; restore is fully reproducible.

## Usage

From the workspace root (the `Justfile` wraps these):

```bash
# Regenerate goldens for every committed fixture.
just goldens

# Force regen even if hashes match.
just goldens-force

# Survey every .vtex_c in Deadlock pak01 (writes tools/format-counts.csv).
just survey

# Pull one entry out of Deadlock into a fixture subdir and regen its golden.
just fixture materials/foo.vtex_c bc7
```

Or invoke `dotnet run` directly from this directory:

```bash
dotnet run -- generate --fixtures ../../morphic/fixtures [--force]
dotnet run -- extract  --vpk PATH --entry NAME --out DIR
dotnet run -- survey   --vpk PATH --out CSV
dotnet run -- model    --vpk PATH --entry NAME [--base PATH] --out GLB
dotnet run -- kv3-dump --vpk PATH --entry NAME --block FOURCC [--nth N] --out JSON [--raw KV3BIN]
```

`model` and `kv3-dump` exist for the `.vmdl_c -> .glb` exporter work (see
`docs/vmdl-glb-exporter-handoff.md`):

- `model` writes a golden glTF via VRF's `GltfModelExporter` (animations on, so
  the skeleton/skin is present for bone-name diffing). The Rust exporter is
  semantically diffed against it. GLBs are large and not committed; regenerate
  with `just model-golden`.
- `kv3-dump` serializes one KV3 block (`DATA`, `MDAT`, ...) to canonical JSON
  for the M1 KV3 parser to diff against, and with `--raw` writes the raw block
  bytes as a committed `morphic/fixtures/kv3/*.kv3bin` fixture. Floats are
  emitted as `{"$f64":"0xHEXBITS"}` (IEEE-754 bit pattern) and blobs as
  `{"$bin":{"len":N,"sha256":"..."}}` so the Rust side matches exactly without
  float-formatting ambiguity. Re-bless with `just kv3-goldens`.

## Why .NET in this repo

The vpkmerge tool itself is pure Rust and stays that way. This is the only
.NET artifact in the workspace and exists strictly to generate goldens. CI
runs `cargo test -p morphic` against the committed `.png` + `.meta.json`
siblings and never invokes `dotnet`. The `source_sha256` field in each
`.meta.json` lets the Rust tests detect stale goldens and tells you to
regenerate.

## `.meta.json` schema

```json
{
  "format": "RGBA8888",
  "width": 16, "height": 16, "depth": 1,
  "mip_count": 1,
  "flags": ["NO_LOD"],
  "source_sha256": "<sha256 of the .vtex_c bytes>",
  "vrf_version": "<VRF assembly version at gen time>",
  "tolerance": { "kind": "byte_exact" }
}
```

`tolerance.kind` is one of `byte_exact`, `mae_u8` (with `epsilon`), or
`hdr_eps` (with `abs` + `rel`). Per-format defaults are picked by
`ToleranceFor()` in `Program.cs`; hand-edit a fixture's meta to override.
