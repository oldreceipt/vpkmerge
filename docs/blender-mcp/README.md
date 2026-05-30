# Deadlock + Blender MCP docs

How we drive **Blender over MCP** to load, inspect, pose, and render Deadlock
assets that `vpkmerge` / `morphic` produce. This is the bridge between the Rust
side (export a `.vmdl_c` to `.glb`) and the Blender side (look at it, pose it,
reskin it, render it).

The pipeline in one line:

```
.vmdl_c  --(vpkmerge model export)-->  .glb  --(blender MCP import)-->  Blender scene  --(render)-->  PNG
```

## Why this folder exists

The Blender MCP is powerful but its viewport-control surface is fiddly with
Deadlock rigs (see the framing trap in the load doc). Each thing we figure out
costs real wall-clock the first time and is cheap to repeat once written down.
This subtree is where those recipes live so the next hero load is a copy-paste,
not a 10-minute rediscovery.

## Layout (nestable)

This folder is meant to grow. Keep it shallow-but-grouped: one subfolder per kind
of doc, kebab-case filenames, one topic per file.

```
docs/blender-mcp/
  README.md                          this index
  workflows/                         step-by-step "how to do X" recipes
    load-hero-into-blender.md        export a hero + import + frame + render
  reference/                         durable facts, gotchas, API notes (add as needed)
  sessions/                          dated logs of specific render/reskin sessions (add as needed)
```

When you add a doc, add a row to the index below and link it from any related
doc. A `reference/foo.md` link that does not exist yet is fine: it marks a doc
worth writing.

## Index

| Doc | What it covers |
|---|---|
| [workflows/load-hero-into-blender.md](workflows/load-hero-into-blender.md) | The full process to get a Deadlock hero (Paige/bookworm) loaded, framed, and rendering in Blender via MCP, including the framing dead-ends and the render-to-PNG workaround. |

## Conventions

- **No em-dashes** anywhere (workspace-wide rule). Colon, period, or parens.
- **Hero model files use codenames, not display names.** Paige = `bookworm`,
  Vindicta = `hornet`, Mina = `vampirebat`. The exporter's `--hero <codename>`
  takes the codename.
- **The `.glb` is a one-way preview.** Nothing reads it back into the game; for
  in-game changes use the `vpkmerge` recolor/edit paths. Blender here is for
  *looking* and *rendering*, not round-tripping geometry (yet).
- **MCP tool calls need a `user_prompt` arg.** `get_scene_info` /
  `execute_blender_code` 422 without it.
- **Scratch GLBs go in `.scratch/`** (gitignored), not committed.

## Related (outside this folder)

- [../handoff-vertex-color-recolor.md](../handoff-vertex-color-recolor.md): the in-game recolor path (Paige ult), and how to find which model a particle actually spawns.
- [../handoff-model-edit.md](../handoff-model-edit.md): why `.glb -> .vmdl_c` does not exist; the geometry pipeline is one-way.
- [../vmdl-glb-exporter.md](../vmdl-glb-exporter.md): the exporter that produces the `.glb` this folder consumes.
- `../../../blender-work/`: the actual `.blend` files (frostline_vindicta, frost_mina, hornet_*).
