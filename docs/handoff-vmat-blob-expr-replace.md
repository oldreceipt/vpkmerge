# Handoff: editing existing dynamic expressions in blob-bearing `.vmat_c`

Date: 2026-06-16. Status: **code done + offline-verified; in-engine partially verified (loads without crashing); uncommitted.**

## What this enables

Editing an *existing* dynamic expression (`m_dynamicParams` bytecode) on a
blob-bearing `.vmat_c` — i.e. `vmat --edit-expr` / `--set-expr` over a param that
already has an expression. Previously refused ("cannot replace an existing dynamic
expression in a blob-bearing material"); the bytecode is a binary blob and only a
blob-aware *replace* could do it. `replace_blob_v5` had that machinery for
`.vnmclip_c` but it was single-blob and had two bugs that surfaced on a real
2-blob material (`ghost2_arm.vmat_c`).

## The two bugs (both fixed in `morphic/src/kv3/rewrap.rs::replace_blob_v5`)

1. **Stale `sizeUncTotal` (DATA offset 48).** When the new blob differs in length,
   `unc2` (offset 80) changes, so offset 48 (= `unc1 + unc2`) must be rewritten. It
   wasn't. morphic's lenient reader recomputes from sub-fields and ignored it; the
   Source 2 engine validates it and rejected the KV3 → **"Bad KV3 data" → crash**.
   Fix: write offset 48 (and a `verify_v5_size_invariants` guard asserts
   `@48==unc1+unc2` and `@52==comp1+comp2` before returning).

2. **Wrong LZ4 frame layout.** The writer concatenated all blobs and chunked the
   region by `frame_size` (16 KB) → one frame for a small region. Valve frames
   **one LZ4 frame per blob** (a 2-blob material ships 2 frames; the reader's
   `decompress_blob_frames` documents this). Our reader decoded either; the engine
   is strict and rejected the single-frame version → second crash. Fix: re-chunk
   per-blob (`for blob in &blobs { for chunk in blob.chunks(frame_size) {...} }`).

After both fixes the edited file is byte-structurally identical in shape to the
vanilla material (same per-blob framing, `@68` frame-table size, consistent header).

## How it was verified

- `cargo test -p morphic -p vpkmerge-core --lib`: morphic 117, core 87, green.
  Regression asserts in `vmat_style::tests::edit_expr_on_blob_material_succeeds`
  (`@48==unc1+unc2`, `@52==comp1+comp2`, `@68==4` i.e. per-blob framing). fmt/clippy clean.
- Field-level proof: dumped vanilla vs edited DATA-block headers; only the fields
  that *should* change differ, all self-consistent.
- **In-engine:** the per-blob-fixed build **loaded without crashing** (the two
  pre-fix builds both crashed with "Bad KV3 data"). So the blob replace is
  engine-loadable. See `citadel/console.log` history for the crash lines.

## Important gotchas discovered

- **VRF / Source2Viewer-CLI is too lenient to gate this.** It parsed *both* crashing
  versions without error. The real Deadlock engine is stricter than VRF on KV3 blob
  framing, so VRF can't pre-validate this class of bug — only the live engine (or the
  `verify_v5_size_invariants`-style structural guards) catch it.
- **`g_vColorTint1` is tint-mask-gated on hero bodies** (only recolors accent regions),
  but full-surface on weapons. A whole-hero `--set-vec g_vColorTint1=...` changed only
  the gun. For an obvious whole-body recolor use the albedo *texture* path
  (`texture`/`trippy-skin` on `g_tColor`), not the tint param. Plain hue-shift also
  keeps saturation, so it barely moves low-saturation skin/dark clothes.

## Open / not done

- **No clean in-engine VISUAL confirmation of the replaced expression running.** The
  test material `ghost2_arm` is a small surface, largely hidden under `ghost2_clothes`
  (the dress), so the pulsing-self-illum edit wasn't clearly visible. Next: drive a
  visible blob-bearing material, or a non-mask-gated param.
- **Addon-mount inconsistency observed in-game** (a magenta-gun test showed once;
  a later galaxy-skin install showed nothing). Looks like addon load-order / cache on
  the install side (`citadel/addons/pakNN_dir.vpk`, managed by a `.dmm.json` mod
  manager), NOT a code bug. Unverified.
- **ZSTD-compressed blobbed blocks still refused** (no ZSTD encoder).
- **Not committed.** Work sits on branch `fix/glb-pure-normal-discriminator`
  (unrelated name). Suggest a dedicated branch `fix/vmat-blob-expr-replace`.

## Files changed

- `morphic/src/kv3/rewrap.rs` — `replace_blob_v5`: write `@48`; per-blob framing;
  `verify_v5_size_invariants` guard.
- `morphic/src/kv3/patch.rs` — `set_blob` accepts unequal length (earlier work).
- `vpkmerge-core/src/vmat_style.rs` — `replace_expr_blob` wiring; test asserts.
- `vpkmerge-cli/src/main.rs` + `vmat_style.rs` — `vmat --list` prints float/vector
  values and a `[dynamic]` tag for blob-bearing materials (the "foundation" change).
- `morphic/src/model/glb.rs` — GLB emissive factor reads `g_vSelfIllumTint1` (the
  "previews" change).
- `CLAUDE.md` — updated the vmat blob-replace caveat.
- `vpkmerge-core/examples/soul_import_clone.rs` — `..Default::default()` (fix a
  pre-existing example compile break so `cargo test --workspace` is green).

## Throwaway tooling left behind (untracked; keep or delete)

- `vpkmerge-core/examples/vmat_marker_census.rs` — full census of shipped `.vmat_c`
  markers (shaders, `F_*` flags + examples, param names). Genuinely useful; worth keeping.
- `vpkmerge-core/examples/dump_entry.rs` — dump one VPK entry to a loose file.
- `morphic/examples/kv3_hdr.rs` — print a resource DATA block's KV3 v5 header fields
  (the vanilla-vs-edited diff tool that found bug #1).

## How to reproduce the test mod

```
vpkmerge vmat --vpk pak01_dir.vpk \
  --entry models/heroes_staging/ghost/materials/ghost2_arm.vmat_c \
  --set-expr 'g_flSelfIllumScale1=10*sin(6*time())+10' \
  --encode-vpk out_dir.vpk
# install: copy out_dir.vpk -> citadel/addons/pakNN_dir.vpk (a free high number),
# fully restart Deadlock. Pre-fix builds crash with "Bad KV3 data"; fixed build loads.
```
