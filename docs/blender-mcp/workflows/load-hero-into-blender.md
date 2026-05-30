# Workflow: load a Deadlock hero into Blender via MCP

Status: **DONE, confirmed.** Paige (bookworm) loaded, textured, rigged, and
rendering correctly. This doc is the as-built record of the process, including the
dead-ends, so the next hero load skips them.

First time through (Paige, 2026-05-29) this took roughly **5 to 10 minutes**, and
almost all of that was one trap: framing the viewport. The export and import were
seconds; the camera/framing dance was the cost. The [Framing trap](#the-framing-trap-where-the-time-went)
section is the part worth reading before you do this again. The
[Fast path](#fast-path-copy-paste) at the bottom is the distilled recipe.

## Prerequisites

- `vpkmerge` built: `cargo build --release -p vpkmerge-cli` (binary at
  `target/release/vpkmerge`).
- Deadlock installed. Main content pak (holds both models and their textures):
  `~/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk`.
- Blender open with the **Blender MCP** addon running (status bar reads
  "Running on port 9876"). Tools used: `mcp__blender__execute_blender_code`,
  `mcp__blender__get_scene_info`, `mcp__blender__get_viewport_screenshot`. All
  require a `user_prompt` arg or they 422.

## The process, step by step

### 1. Find the model and confirm the toolchain (seconds)

The exporter has a `--hero <codename>` mode that auto-discovers a hero's body
model (`<dir>/<codename>.vmdl_c`), so you do not need to know the exact VPK path.
Paige's codename is `bookworm`.

```
target/release/vpkmerge model export --help     # confirm export flags
```

Locate the pak (once, then reuse the path):

```
find ~ -iname pak01_dir.vpk 2>/dev/null | grep '/citadel/'
# -> ~/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk
```

Confirm Blender is reachable (note the required `user_prompt`):

```python
# mcp__blender__execute_blender_code, user_prompt="connectivity test"
import sys; print("blender python alive", sys.version.split()[0])
```

### 2. Export the hero body to GLB (seconds)

`--no-anim` gives a clean rigged static mesh (skeleton, no clips). The exporter
bakes the textures into the `.glb`.

```
PAK="$HOME/.local/share/Steam/steamapps/common/Deadlock/game/citadel/pak01_dir.vpk"
mkdir -p .scratch
target/release/vpkmerge model export \
  --vpk "$PAK" --hero bookworm --no-anim \
  --out .scratch/paige_bookworm.glb
# -> wrote .scratch/paige_bookworm.glb   (~36 MB)
```

Want a posed static mesh instead of a T-pose? Swap `--no-anim` for
`--pose [CLIP@FRAME]` (omit the clip to try the default menu/idle poses). That
drops the skeleton and bakes one frame into the mesh.

### 3. Import into Blender (seconds)

```python
# user_prompt="import Paige bookworm GLB"
import bpy
if "Cube" in bpy.data.objects:                       # drop the default cube
    bpy.data.objects.remove(bpy.data.objects["Cube"], do_unlink=True)
bpy.ops.import_scene.gltf(filepath="/home/esoc/grimoire-workspace/vpkmerge/.scratch/paige_bookworm.glb")
```

What Paige came in as (a useful sanity baseline for any hero body):

- **3 meshes**: `bookworm` (body, 28,749 verts), `head` (18,434 verts), plus the
  eyes mesh.
- **1 armature** named `skeleton`, **164 bones**.
- **8 materials**, all `bookworm_*` / `boomworm_*`, each with image textures bound
  (hair, head, eyes, lens, upper/lower outfit, teeth, books).
- True size, measured later: ~1.27 m wide x ~2.3 m tall, standing upright,
  centered around z = 1.07.

If the counts and `bookworm_*` material names show up, you have the right hero.

### 4. Frame and verify (this is where the time goes)

See the next section. The short version: do **not** trust the viewport-framing
operators here; render an ortho camera to a PNG and read that.

## The framing trap (where the time went)

Every "frame the model" shortcut failed in a confusing way. Root cause:

> The imported **armature has scale 0.025**, and the meshes are parented to it.
> Their object-level bounds report at the parent's tiny scale with origins pinned
> at the world origin (0, 0, 0). So Blender's framing operators compute a bounding
> box that is either tiny or centered inside the geometry.

Symptoms seen, in order, so you recognize them:

1. **`bpy.ops.view3d.view_selected()` zoomed into a flat grey wall.** That grey is
   the inside of the mesh seen point-blank; the operator zoomed to a near-zero box
   at the origin. The grid read "10 Centimeters", a tell that the view is microscopic.
2. **Every `get_viewport_screenshot` looked identical** even after I changed the
   view. Two causes: (a) my `region_3d` writes targeted `bpy.context.screen`
   instead of iterating `bpy.context.window_manager.windows`, so they hit a
   different window than the one being captured; (b) screenshots can lag a frame.
   Fix: iterate the real WM windows, and use a full `temp_override(window=, area=,
   region=)` for any viewport operator.
3. **`view_all` finally framed something, but it was a stray ~2 m Icosphere** at
   the origin (not part of the export; the import only created `bookworm` + `head`
   nodes), which is bigger than Paige and dominated the box. Fix: remove the stray
   object, then frame only `bookworm` + `head`.
4. **A clever render-engine auto-detect crashed** (`__subclasses__()` walked into
   `HydraRenderEngine`, which has no `identifier`). Fix: just set the engine string
   directly, do not introspect.

### Ground truth: measure, do not trust the operators

Read the real world-space bounds through the depsgraph (this respects the armature
deform and gives honest numbers):

```python
# user_prompt="measure Paige world bounds"
import bpy, mathutils
deps = bpy.context.evaluated_depsgraph_get()
mn = mathutils.Vector((1e18,)*3); mx = mathutils.Vector((-1e18,)*3)
for nm in ("bookworm", "head"):
    ob = bpy.data.objects[nm].evaluated_get(deps)
    me = ob.to_mesh()
    for v in me.vertices:
        w = ob.matrix_world @ v.co
        for i in range(3):
            mn[i] = min(mn[i], w[i]); mx[i] = max(mx[i], w[i])
    ob.to_mesh_clear()
center = (mn + mx) / 2; size = mx - mn
print("center", tuple(round(v,3) for v in center), "size", tuple(round(v,3) for v in size))
# -> center (0.0, 0.056, 1.066)  size (1.268, 0.732, 2.298)
```

That confirmed the geometry was fine all along: 2.3 m tall, upright, centered at
z ~ 1.07. The problem was never the mesh, only the framing.

### The reliable fix: render an ortho camera to a PNG

Skip the viewport entirely for verification. Place an orthographic camera using
the measured center/height, add a sun, neutralize the world, and render to a file
you read directly. This is ground truth and never lies.

```python
# user_prompt="render Paige front view to PNG"
import bpy, mathutils, math
scene = bpy.context.scene
scene.render.engine = 'BLENDER_EEVEE'   # 4.x may expose 'BLENDER_EEVEE_NEXT'; set the string, do not introspect

center = mathutils.Vector((0.0, 0.056, 1.066)); height = 2.298

cam = bpy.data.objects.get("Camera") or bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
if cam.name not in scene.collection.objects:
    scene.collection.objects.link(cam)
cam.data.type = 'ORTHO'
cam.data.ortho_scale = height * 1.15
cam.location = (center.x, center.y - 6.0, center.z)
cam.rotation_euler = (math.radians(90), 0.0, 0.0)   # look down +Y, +Z up = front view
scene.camera = cam

key = bpy.data.objects.get("KeyLight") or bpy.data.objects.new("KeyLight", bpy.data.lights.new("KeyLight", 'SUN'))
if key.name not in scene.collection.objects:
    scene.collection.objects.link(key)
key.data.energy = 3.0
key.rotation_euler = (math.radians(55), math.radians(10), math.radians(30))

world = scene.world or bpy.data.worlds.new("World"); scene.world = world
world.use_nodes = True
bg = world.node_tree.nodes.get("Background")
if bg:
    bg.inputs[0].default_value = (0.08, 0.08, 0.09, 1.0); bg.inputs[1].default_value = 0.6

scene.render.resolution_x = 800; scene.render.resolution_y = 1000
scene.render.filepath = "/tmp/paige_front.png"
bpy.ops.render.render(write_still=True)
```

Then read `/tmp/paige_front.png` directly. That render is what confirmed Paige:
orange hair, round glasses, green academic dress with stockings, open book at her
feet.

### Leave the user's viewport clean

So the live Blender window looks right when they tab over:

```python
# user_prompt="clean viewport for the user"
import bpy
arm = bpy.data.objects.get("skeleton")
if arm: arm.hide_set(True)                  # hide the bone clutter
for window in bpy.context.window_manager.windows:
    for area in window.screen.areas:
        if area.type == 'VIEW_3D':
            sp = area.spaces.active
            sp.shading.type = 'MATERIAL'
            sp.region_3d.view_perspective = 'CAMERA'   # use the framed camera
```

## Fast path (copy-paste)

For the next hero, in order. Replace `bookworm` with the target
[codename](../README.md#conventions) and adjust the measured `center`/`height`
after step 3.

1. **Export:**
   `target/release/vpkmerge model export --vpk "$PAK" --hero <codename> --no-anim --out .scratch/<name>.glb`
   (`$PAK` = `.../game/citadel/pak01_dir.vpk`)
2. **Import:** `bpy.ops.import_scene.gltf(filepath=".../.scratch/<name>.glb")` after
   removing the default cube.
3. **Measure** world bounds via the depsgraph snippet above (do not trust
   `view_selected`).
4. **Render** an ortho camera to `/tmp/<name>_front.png` with the camera snippet,
   read the PNG to verify.
5. **Clean** the viewport (hide armature, switch to camera view).

## Pitfalls checklist

- [ ] Armature scale is 0.025; framing operators lie. Measure via depsgraph, verify via render.
- [ ] Iterate `window_manager.windows` for viewport edits; `bpy.context.screen` may be the wrong window.
- [ ] Remove any stray scene object before `view_all` (it can dominate the frame).
- [ ] Set the render engine string directly; do not introspect `RenderEngine.__subclasses__()`.
- [ ] MCP calls need `user_prompt`.
- [ ] `.glb` is preview-only; in-game changes go through the `vpkmerge` recolor/edit paths, not this.
