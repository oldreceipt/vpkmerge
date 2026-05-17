# Fixtures

Two tiers:

- **Canonical (committed):** small textures grouped by format under
  `<format>/`. Each ships with a sibling `.png` (oracle output) and
  `.meta.json` (format, dims, source SHA, tolerance). The Rust golden harness
  diffs morphic's decode against these and verifies the source SHA, so CI is
  immune to silent fixture drift.

- **Local (gitignored):** `_local/` holds the extended corpus extracted from
  the user's Deadlock install via `just bootstrap`. Used for stress testing
  before declaring a milestone done. Not in CI.

## Layout

```
fixtures/
  rgba8/                  RGBA8888 + BGRA8888 (M2)
  dxt1/                   DXT1 = BC1 (M3)
  dxt5/                   DXT5 = BC3 (M4)
  ati1n/, ati2n/          BC4, BC5 (M5)
  bc7/                    (M6)
  bc6h/                   (M7)
  rgba16f/                (M8)
  _local/                 gitignored, oracle-populated stress corpus
```

## Provenance

| Path                                          | Source (Deadlock pak01 entry)                                        |
|-----------------------------------------------|----------------------------------------------------------------------|
| `rgba8/minimap_circle.vtex_c`                 | `materials/minimap/minimap_circle.vtex_c`                            |
| `dxt1/default_color_tga_99901565.vtex_c`      | `materials/default/default_color_tga_99901565.vtex_c`                |

Extracted via `tools/morphic-oracle` from a local Steam install. Re-extract
with `just fixture <entry> <subdir>`. See `tools/format-counts.csv` for the
full pak01 inventory.

## Copyright

The committed canonical corpus is intentionally tiny so it serves as a
regression test, not redistribution. Where possible, prefer hand-authored
synthetic textures (gradient, checker, alpha edge) over game assets. The
extended `_local/` corpus is per-user and never leaves the developer's
machine.
