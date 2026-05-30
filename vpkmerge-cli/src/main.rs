use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use vpkmerge_core::{
    edit_model_geometry, export_hero_model, export_model, extract_portraits, inspect_models, merge,
    model_draw_call_targets, model_vertex_targets, split, AnimOptions, CollisionPolicy,
    GeometryEdit, MergeOptions, OverlapPolicy, PathPredicate, PortraitInfo, PoseSelection,
    SoundEvents, SplitOptions, SplitOutput,
};

#[derive(Parser)]
#[command(
    name = "vpkmerge",
    version,
    about = "Combine multiple VPK files into one (or split one into many).",
    long_about = "Combine multiple Valve Pak (VPK) files into one. Built for Deadlock \
                  modding so players can consolidate several mods into a single addon VPK.\n\
                  \n\
                  By default, later inputs win on path collision. Use --strict to refuse to \
                  merge when paths collide.\n\
                  \n\
                  See `vpkmerge split --help` for the inverse operation (one VPK to many).",
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Command>,

    /// Output VPK path. Parent directory is created if missing.
    output: Option<PathBuf>,

    /// Input VPK paths. At least 2 required (validated by the engine).
    #[arg(num_args = 0..)]
    inputs: Vec<PathBuf>,

    /// Error out on any path collision instead of letting later inputs win.
    #[arg(long)]
    strict: bool,

    /// Print each path that gets overridden by a later input.
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Route entries from one VPK into N output VPKs by path predicate.
    Split(SplitCmd),

    /// Extract and decode hero portrait/card art from a VPK to PNG.
    Portrait(PortraitCmd),

    /// Inspect compiled models (`.vmdl_c`) in a VPK: block structure,
    /// mesh-part count, embedded geometry, and skeleton/physics presence.
    Model(ModelCmd),

    /// Decode a soundevents (`.vsndevts_c`) file to JSON, optionally edit clip
    /// paths / params and re-emit an uncompressed (loadable) file.
    Soundevents(SoundeventsCmd),

    /// Recolor a texture (`.vtex_c`) by shifting every pixel's hue to a target
    /// (keeping saturation + value), then re-encode in the texture's own
    /// format. Built for the Deadlock ability-VFX recolor: pack the result at
    /// the base entry path and it overrides in place, no `.vmat_c` edit needed.
    Texture(TextureCmd),

    /// Recolor a hero's full ability VFX (particles + color textures + baked
    /// vertex colors) to one hue and pack it into a single addon VPK. The
    /// one-call bridge for a mod manager: composes all three recolor mechanisms
    /// over a built-in per-hero recipe (Paige / `bookworm` for now).
    RecolorHero(RecolorHeroCmd),
}

#[derive(Args)]
struct SoundeventsCmd {
    /// Path to a `.vsndevts_c` file, or (with --from-vpk) an entry path inside a VPK.
    input: PathBuf,

    /// Read INPUT as an entry path inside this VPK instead of a file on disk
    /// (e.g. `--from-vpk pak01_dir.vpk soundevents/hero/gigawatt.vsndevts_c`).
    #[arg(long, value_name = "VPK")]
    from_vpk: Option<PathBuf>,

    /// After applying edits, re-encode (uncompressed v4) to this loose file.
    #[arg(long, value_name = "FILE")]
    encode: Option<PathBuf>,

    /// After applying edits, re-encode and pack into a standalone VPK at this
    /// path. The encoded file lands at its VPK entry path (see --vpk-entry).
    #[arg(long = "encode-vpk", value_name = "OUT_dir.vpk")]
    encode_vpk: Option<PathBuf>,

    /// Entry path for the file inside --encode-vpk. Defaults to INPUT when
    /// reading with --from-vpk; required for a loose-file INPUT.
    #[arg(long = "vpk-entry", value_name = "PATH")]
    vpk_entry: Option<String>,

    /// Replace a clip path everywhere in the tree: --swap-vsnd OLD=NEW (repeatable).
    #[arg(long = "swap-vsnd", value_name = "OLD=NEW")]
    swap_vsnd: Vec<String>,

    /// Set a numeric field on one event: --set EVENT/FIELD=NUMBER (repeatable),
    /// e.g. --set "Seven.Wpn.Fire/volume=0.25".
    #[arg(long = "set", value_name = "EVENT/FIELD=NUMBER")]
    set: Vec<String>,
}

#[derive(Args)]
struct TextureCmd {
    /// One or more `.vtex_c` files, or (with --from-vpk) entry paths inside a
    /// VPK. Pass several to recolor a whole set into one addon (--encode-vpk).
    #[arg(required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,

    /// Read INPUTS as entry paths inside this VPK instead of files on disk
    /// (e.g. `--from-vpk pak01_dir.vpk models/.../bookworm_dragon_color.vtex_c`).
    #[arg(long, value_name = "VPK")]
    from_vpk: Option<PathBuf>,

    /// Target hue in degrees (0..360). Every pixel is set to this hue while
    /// keeping its original saturation and value, so neutral highlights and
    /// shadows stay neutral. Matches the particle recolor, so the same hue
    /// lands the dragon, the projectile, and the particle params together.
    #[arg(long, value_name = "DEG", allow_hyphen_values = true)]
    hue: f64,

    /// Saturation scale (default 1.0 = keep source). > 1 lifts pale, washed-out
    /// areas toward the picked color; < 1 mutes them toward a pastel.
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    saturation: f64,

    /// Brightness (HSV value) scale (default 1.0 = keep source). > 1 lightens
    /// (e.g. a light/pastel color), < 1 darkens (a deep/ink color).
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    brightness: f64,

    /// Write a PNG preview of the recolored top mip here (the design-intent
    /// color, before the lossy `BCn` re-encode) for an eyeball before committing.
    /// Single input only.
    #[arg(long, value_name = "PNG")]
    preview: Option<PathBuf>,

    /// Write the recolored `.vtex_c` to this loose file. Single input only.
    #[arg(long, value_name = "FILE")]
    encode: Option<PathBuf>,

    /// After recoloring, pack into a standalone addon VPK at this path. With
    /// several INPUTS, all recolored textures land in this one addon, each at
    /// its VPK entry path (see --vpk-entry for the single loose-file case).
    #[arg(long = "encode-vpk", value_name = "OUT_dir.vpk")]
    encode_vpk: Option<PathBuf>,

    /// Entry path for the file inside --encode-vpk. Defaults to INPUT when
    /// reading with --from-vpk; required for a loose-file INPUT. Single input only.
    #[arg(long = "vpk-entry", value_name = "PATH")]
    vpk_entry: Option<String>,
}

#[derive(Args)]
struct RecolorHeroCmd {
    /// Hero model/particle codename to recolor (e.g. `bookworm` for Paige). Only
    /// heroes with a pinned recipe are supported so far.
    #[arg(long, value_name = "CODENAME")]
    hero: String,

    /// VPK to read the hero's VFX from (a skin VPK, or the base pak itself).
    #[arg(long, value_name = "VPK")]
    vpk: PathBuf,

    /// Base `pak01_dir.vpk` to fall back to for any entry `--vpk` does not ship
    /// (so a texture-only skin still recolors the base mesh/particles).
    #[arg(long, value_name = "VPK")]
    base: Option<PathBuf>,

    /// Target hue in degrees (0..360). Every color is set to this hue while
    /// keeping its saturation and value, so one value lands particles, textures,
    /// and vertex colors on the same color.
    #[arg(long, value_name = "DEG", allow_hyphen_values = true)]
    hue: f64,

    /// Saturation scale (default 1.0 = keep source). > 1 lifts pale, washed-out
    /// areas toward the picked color; < 1 mutes them toward a pastel. Applied to
    /// particles, textures, and vertex colors alike.
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    saturation: f64,

    /// Brightness (HSV value) scale (default 1.0 = keep source). > 1 lightens
    /// (e.g. a light/pastel color), < 1 darkens (a deep/ink color).
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    brightness: f64,

    /// Pack the whole recolored VFX set into this one addon VPK, each entry at
    /// its base path so it overrides the base game in place. Required unless
    /// `--preview-png` is given.
    #[arg(
        long = "encode-vpk",
        value_name = "OUT_dir.vpk",
        required_unless_present = "preview_png"
    )]
    encode_vpk: Option<PathBuf>,

    /// Skip the (slow) full bake and instead write a fast PNG swatch of the
    /// recipe's representative ability texture recolored to this target, for a
    /// live UI preview. Mutually exclusive use with `--encode-vpk` is allowed
    /// (preview wins and the bake is skipped).
    #[arg(long = "preview-png", value_name = "PNG")]
    preview_png: Option<PathBuf>,
}

#[derive(Args)]
struct PortraitCmd {
    /// Input VPK to read portraits from.
    input: PathBuf,

    /// Directory to write decoded PNGs into (created if missing).
    #[arg(long, value_name = "DIR")]
    out: PathBuf,

    /// Only extract portraits for this hero codename (e.g. "hornet").
    /// Omit to extract every hero in the VPK.
    #[arg(long, value_name = "CODENAME")]
    hero: Option<String>,

    /// Write the JSON manifest to this file instead of stdout.
    #[arg(long, value_name = "FILE")]
    manifest: Option<PathBuf>,
}

#[derive(Args)]
struct ModelCmd {
    /// VPK to inspect (block structure, mesh/skeleton presence). Omit when
    /// using a subcommand such as `export`.
    input: Option<PathBuf>,

    #[command(subcommand)]
    action: Option<ModelAction>,
}

#[derive(Subcommand)]
enum ModelAction {
    /// Export a model entry to a textured binary glTF (`.glb`).
    Export(ModelExportArgs),

    /// Edit a model and pack the result into an addon VPK. Reshape existing
    /// geometry (vertex displacement via `--scale`/`--translate` or a Blender
    /// `--from-glb`), remove a part by material (`--remove-material`), or replace a
    /// part's geometry wholesale with a new mesh of any vertex/index count
    /// (`--replace-part <MESH> --from-glb <FILE>`, Tier 1d).
    Edit(ModelEditArgs),

    /// Recolor a model's baked per-vertex `COLOR` to a target hue (keeping
    /// saturation + value), re-encode the affected vertex buffers, and pack the
    /// result into an addon VPK. The model half of the Deadlock ability-VFX
    /// recolor: some effects (Paige's ult horse/knight) bake their color into
    /// mesh vertex colors, which the particle/texture recolors don't reach. Pass
    /// several ENTRIES to recolor a whole set into one addon, each overriding its
    /// base entry in place. Mirrors `vpkmerge texture`.
    Recolor(ModelRecolorArgs),
}

#[derive(Args)]
struct ModelRecolorArgs {
    /// One or more VPK-internal model paths (`.vmdl_c`) to recolor, read from
    /// `--vpk`. Several ENTRIES pack into one addon, each at its own path, e.g.
    /// `models/heroes_wip/bookworm/bookworm_horse.vmdl_c`.
    #[arg(required = true, num_args = 1.., value_name = "ENTRY")]
    entries: Vec<String>,

    /// VPK to read the model(s) from (a skin VPK, or the base pak itself).
    #[arg(long, value_name = "VPK")]
    vpk: PathBuf,

    /// Base `pak01_dir.vpk` to fall back to when `--vpk` does not ship a model.
    #[arg(long, value_name = "VPK")]
    base: Option<PathBuf>,

    /// Target hue in degrees (0..360). Every vertex's color is set to this hue
    /// while keeping its saturation and value, so neutral vertices stay neutral.
    /// Matches the particle/texture recolor: the same hue lands all three.
    #[arg(long, value_name = "DEG", allow_hyphen_values = true)]
    hue: f64,

    /// Saturation scale (default 1.0 = keep source). > 1 boosts, < 1 mutes.
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    saturation: f64,

    /// Brightness (HSV value) scale (default 1.0 = keep source). > 1 lightens,
    /// < 1 darkens.
    #[arg(long, value_name = "SCALE", default_value_t = 1.0)]
    brightness: f64,

    /// List each model's color-bearing vertex buffers (mesh part, block, vertex
    /// count) and exit without recoloring.
    #[arg(long)]
    list: bool,

    /// Pack the recolored model(s) into a standalone addon VPK at this path,
    /// each at its base entry path so it overrides in place.
    #[arg(long = "encode-vpk", value_name = "OUT_dir.vpk")]
    encode_vpk: Option<PathBuf>,
}

#[derive(Args)]
struct ModelExportArgs {
    /// VPK containing the `.vmdl_c` (a skin VPK, or the base pak itself).
    #[arg(long, value_name = "VPK")]
    vpk: PathBuf,

    /// VPK-internal model path, e.g.
    /// `models/heroes_staging/hornet_v3/hornet.vmdl_c`. Mutually exclusive with
    /// `--hero`.
    #[arg(
        long,
        value_name = "PATH",
        required_unless_present = "hero",
        conflicts_with = "hero"
    )]
    entry: Option<String>,

    /// Hero codename (e.g. `hornet`, `bookworm`) whose body model
    /// (`<dir>/<codename>.vmdl_c`) is auto-discovered. Mutually exclusive with
    /// `--entry`.
    #[arg(long, value_name = "CODENAME")]
    hero: Option<String>,

    /// Base `pak01_dir.vpk` that referenced materials/textures resolve against
    /// when the skin VPK does not ship them.
    #[arg(long, value_name = "VPK")]
    base: Option<PathBuf>,

    /// Exclude all animation clips (export the static mesh + skeleton only).
    #[arg(long, conflicts_with = "clip")]
    no_anim: bool,

    /// Only export the named clip(s) (e.g. `--clip primary_stand_idle`).
    /// Repeatable. Omit to export every clip the model carries.
    #[arg(long, value_name = "NAME")]
    clip: Vec<String>,

    /// Bake a single frame into the mesh as a static posed `.glb` (no skeleton,
    /// skin, or clips). Optional value `CLIP[@FRAME]`: omit the clip to try the
    /// default menu/idle poses, append `@N` to pick a frame (default 0). E.g.
    /// `--pose`, `--pose ui_hero_pose`, `--pose idle_loadout@5`.
    #[arg(
        long,
        value_name = "CLIP[@FRAME]",
        num_args = 0..=1,
        default_missing_value = "",
        conflicts_with_all = ["no_anim", "clip"]
    )]
    pose: Option<String>,

    /// With `--pose`, error out instead of emitting a static bind/T-pose when the
    /// model carries no menu/idle pose clip (WIP heroes ship the rig but no baked
    /// clips). Lets a caller fall back to a 2D portrait rather than show an
    /// unposed hero.
    #[arg(long, requires = "pose")]
    require_pose: bool,

    /// Output `.glb` path.
    #[arg(long, value_name = "FILE")]
    out: PathBuf,
}

#[derive(Args)]
struct ModelEditArgs {
    /// VPK containing the `.vmdl_c` to edit (a mesh skin, or the base pak itself).
    #[arg(long, value_name = "VPK")]
    vpk: PathBuf,

    /// VPK-internal model path, e.g.
    /// `models/heroes_staging/hornet_v3/hornet.vmdl_c`.
    #[arg(long, value_name = "PATH")]
    entry: String,

    /// Base `pak01_dir.vpk` to read the model from when `--vpk` is a texture-only
    /// skin that does not ship the mesh. Rarely needed for a geometry edit.
    #[arg(long, value_name = "VPK")]
    base: Option<PathBuf>,

    /// List the model's editable vertex buffers (mesh part, block index, vertex
    /// count) and exit without editing.
    #[arg(long)]
    list: bool,

    /// List the model's renderable draw calls (mesh part, material, vertex/index
    /// count) and exit. Use it to find the material for `--remove-material`.
    #[arg(long = "list-drawcalls")]
    list_drawcalls: bool,

    /// Remove every draw call whose material contains this string
    /// (case-insensitive), so that part stops rendering, then pack an addon VPK
    /// (needs `--encode-vpk`). E.g. `--remove-material vindicta_dress`. This is a
    /// draw-call-only edit: no vertices change.
    #[arg(long = "remove-material", value_name = "MATERIAL")]
    remove_material: Option<String>,

    /// Diagnostic: re-encode every MDAT block unchanged and pack an addon VPK
    /// (needs `--encode-vpk`). Probes whether the engine accepts our re-encoded
    /// model KV3 blocks at all, independent of any edit.
    #[arg(long = "reencode-mdat")]
    reencode_mdat: bool,

    /// Edit only the mesh part with this name (see `--list`). For the transform
    /// edit, omit to edit every editable part. For `--export-glb` / `--from-glb`
    /// the part must resolve to a single buffer (else pass `--block`).
    #[arg(long, value_name = "NAME")]
    part: Option<String>,

    /// Target a specific vertex buffer by its block index (see `--list`).
    /// Disambiguates a multi-buffer part for `--export-glb` / `--from-glb`.
    #[arg(long, value_name = "INDEX")]
    block: Option<usize>,

    /// Export the chosen buffer to a `.glb` (with a `_ORIGID` carrier) for
    /// reshaping in Blender, then re-import with `--from-glb`. Needs `--part`
    /// (single-buffer) or `--block`.
    #[arg(long = "export-glb", value_name = "FILE", conflicts_with_all = ["from_glb", "list"])]
    export_glb: Option<PathBuf>,

    /// Apply a Blender-reshaped `.glb` (from `--export-glb`) back onto the buffer
    /// and pack an addon VPK (needs `--encode-vpk`). Topology must be preserved.
    #[arg(long = "from-glb", value_name = "FILE", conflicts_with_all = ["export_glb", "list"])]
    from_glb: Option<PathBuf>,

    /// Replace the named mesh part's geometry with a new mesh from `--from-glb`
    /// (Tier 1d), then pack an addon VPK (needs `--encode-vpk`). The new mesh may
    /// have any vertex/index count but must be skinned to bones the target part
    /// already uses; the part must be single-buffer/single-drawcall (e.g. `gun`).
    #[arg(
        long = "replace-part",
        value_name = "MESH",
        conflicts_with_all = ["export_glb", "list", "list_drawcalls", "remove_material", "reencode_mdat"]
    )]
    replace_part: Option<String>,

    /// Disambiguates which primitive `--from-glb` provides when replacing a part
    /// from a multi-mesh `.glb` (defaults to the only primitive).
    #[arg(long = "glb-mesh", value_name = "NAME")]
    glb_mesh: Option<String>,

    /// Uniform scale about each edited part's centroid (1.0 = unchanged).
    #[arg(long, default_value_t = 1.0, value_name = "S")]
    scale: f32,

    /// Translate edited geometry by `x,y,z` in model space (applied after scale).
    #[arg(long, value_name = "x,y,z")]
    translate: Option<String>,

    /// Output addon VPK. Packs the edited model at `--entry` (or `--vpk-entry`)
    /// so it overrides the base pak.
    #[arg(long = "encode-vpk", value_name = "OUT_dir.vpk")]
    encode_vpk: Option<PathBuf>,

    /// Entry path for the edited model inside `--encode-vpk` (defaults to `--entry`).
    #[arg(long, value_name = "PATH")]
    vpk_entry: Option<String>,
}

#[derive(Args)]
struct SplitCmd {
    /// Input VPK to split.
    input: PathBuf,

    /// JSON plan describing outputs and prefix predicates.
    #[arg(long, value_name = "FILE")]
    plan: PathBuf,

    /// Refuse to split if any path matches more than one output.
    #[arg(long, conflicts_with = "all_matches")]
    strict: bool,

    /// Route each path to EVERY matching output (entries can land in multiple outputs).
    #[arg(long)]
    all_matches: bool,

    /// Optional residual VPK path. Overrides "residual" in the plan if both are set.
    #[arg(long, value_name = "FILE")]
    residual: Option<PathBuf>,

    /// Log each path routed to each output.
    #[arg(short, long)]
    verbose: bool,
}

#[derive(serde::Deserialize)]
struct PlanFile {
    outputs: Vec<PlanOutput>,
    #[serde(default)]
    residual: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct PlanOutput {
    path: PathBuf,
    prefixes: Vec<String>,
}

#[derive(serde::Serialize)]
struct Manifest {
    input: String,
    portraits: Vec<ManifestEntry>,
}

#[derive(serde::Serialize)]
struct ManifestEntry {
    source_path: String,
    hero_codename: String,
    variant: String,
    width: u32,
    height: u32,
    format_name: String,
    /// Absolute path to the decoded PNG, or null if not decoded.
    output_path: Option<String>,
    /// Why the texture was skipped (null when decoded).
    skipped_reason: Option<String>,
}

impl From<PortraitInfo> for ManifestEntry {
    fn from(p: PortraitInfo) -> Self {
        Self {
            source_path: p.source_path,
            hero_codename: p.hero_codename,
            variant: p.variant.as_str().to_string(),
            width: p.width,
            height: p.height,
            format_name: p.format_name.to_string(),
            output_path: p.output_path.map(|p| p.display().to_string()),
            skipped_reason: p.skipped_reason,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Some(Command::Split(args)) => run_split(args),
        Some(Command::Portrait(args)) => run_portrait(args),
        Some(Command::Model(args)) => run_model(args),
        Some(Command::Soundevents(args)) => run_soundevents(args),
        Some(Command::Texture(args)) => run_texture(args),
        Some(Command::RecolorHero(args)) => run_recolor_hero(&args),
        None => run_merge(cli),
    }
}

/// Parses a `--pose` value `CLIP[@FRAME]` into a [`PoseSelection`]. An empty clip
/// part means "use the default candidate poses"; the frame defaults to 0.
fn parse_pose(spec: &str) -> Result<PoseSelection> {
    let (clip, frame) = match spec.split_once('@') {
        Some((c, f)) => (
            c,
            f.parse::<usize>()
                .with_context(|| format!("--pose frame `{f}` is not a non-negative integer"))?,
        ),
        None => (spec, 0),
    };
    let clips = if clip.is_empty() {
        Vec::new()
    } else {
        vec![clip.to_string()]
    };
    Ok(PoseSelection {
        clips,
        frame,
        require: false,
    })
}

fn run_model(args: ModelCmd) -> Result<()> {
    match args.action {
        Some(ModelAction::Export(e)) => return run_model_export(&e),
        Some(ModelAction::Edit(e)) => return run_model_edit(e),
        Some(ModelAction::Recolor(e)) => return run_model_recolor(&e),
        None => {}
    }

    let input = args.input.context(
        "model: provide a VPK to inspect, or use `model export --vpk <vpk> --entry <path> --out <file.glb>`",
    )?;
    let models = inspect_models(&input)
        .with_context(|| format!("inspecting models in {}", input.display()))?;

    if models.is_empty() {
        println!("{}: no .vmdl_c entries found", input.display());
        return Ok(());
    }

    for m in &models {
        let i = &m.info;
        println!("\n{}", m.path);
        println!(
            "  mesh parts: {}   index buffers: {}   vertex bytes: {}",
            i.mesh_parts, i.index_buffers, i.vertex_bytes
        );
        println!(
            "  embedded geometry: {}   skeleton/anim: {}   physics: {}",
            i.has_embedded_geometry, i.has_skeleton_anim, i.has_physics
        );

        // Collapse the block table into a "KINDxCOUNT (bytes)" histogram so a
        // model with 8 MVTX/MIDX pairs reads at a glance instead of 29 lines.
        let mut counts: std::collections::BTreeMap<&str, (usize, u64)> =
            std::collections::BTreeMap::new();
        for b in &i.blocks {
            let e = counts.entry(b.kind.as_str()).or_insert((0, 0));
            e.0 += 1;
            e.1 += u64::from(b.size);
        }
        let histogram: Vec<String> = counts
            .iter()
            .map(|(k, (n, sz))| format!("{k}x{n} ({sz}B)"))
            .collect();
        println!("  blocks: {}", histogram.join("  "));
    }

    Ok(())
}

fn run_model_export(e: &ModelExportArgs) -> Result<()> {
    let pose = e.pose.as_deref().map(parse_pose).transpose()?.map(|mut p| {
        p.require = e.require_pose;
        p
    });
    let anim = AnimOptions {
        no_anim: e.no_anim,
        clips: e.clip.clone(),
        pose,
    };
    match (&e.entry, &e.hero) {
        (Some(entry), _) => export_model(&e.vpk, entry, e.base.as_deref(), &anim, &e.out)
            .with_context(|| format!("exporting {entry} from {}", e.vpk.display()))?,
        (None, Some(hero)) => export_hero_model(&e.vpk, hero, e.base.as_deref(), &anim, &e.out)
            .with_context(|| format!("exporting hero {hero} from {}", e.vpk.display()))?,
        (None, None) => anyhow::bail!("model export: provide --entry or --hero"),
    }
    println!("wrote {}", e.out.display());
    Ok(())
}

fn run_model_edit(e: ModelEditArgs) -> Result<()> {
    // --list: enumerate vertex buffers and exit.
    if e.list {
        return run_model_list_targets(&e);
    }

    // --list-drawcalls: enumerate renderable draw calls and exit.
    if e.list_drawcalls {
        return run_model_list_drawcalls(&e);
    }

    // --remove-material: drop matching draw calls and pack an addon VPK.
    if e.remove_material.is_some() {
        return run_model_remove_material(&e);
    }

    // --reencode-mdat: diagnostic identity re-encode of every MDAT block.
    if e.reencode_mdat {
        return run_model_reencode_mdat(&e);
    }

    // --export-glb: write the chosen buffer to a glb for Blender editing.
    if let Some(out_glb) = &e.export_glb {
        let block = resolve_edit_block(&e)?;
        vpkmerge_core::export_model_buffer_glb(&e.vpk, &e.entry, e.base.as_deref(), block, out_glb)
            .with_context(|| format!("exporting buffer {block} of {}", e.entry))?;
        println!("wrote {} (block {block} of {})", out_glb.display(), e.entry);
        return Ok(());
    }

    // --replace-part: splice a new mesh (from --from-glb) over a part and pack.
    if let Some(mesh_name) = &e.replace_part {
        return run_model_replace_part(&e, mesh_name);
    }

    // --from-glb: apply a Blender-reshaped glb back and pack an addon VPK.
    if let Some(in_glb) = &e.from_glb {
        let block = resolve_edit_block(&e)?;
        let out = e.encode_vpk.as_ref().context(
            "model edit --from-glb: provide --encode-vpk <OUT_dir.vpk> for the edited model",
        )?;
        let glb_bytes =
            std::fs::read(in_glb).with_context(|| format!("reading {}", in_glb.display()))?;
        let vpk_entry = vpkmerge_core::apply_model_edit_glb(
            &e.vpk,
            &e.entry,
            e.base.as_deref(),
            block,
            &glb_bytes,
            out,
            e.vpk_entry.as_deref(),
        )
        .with_context(|| format!("applying {} to {}", in_glb.display(), e.entry))?;
        println!(
            "wrote {}: 1 entry ({vpk_entry}) reshaped from {}",
            out.display(),
            in_glb.display()
        );
        return Ok(());
    }

    // Guard against a no-op (which would just repack the model unchanged): no
    // scale change and no translate given.
    if (e.scale - 1.0).abs() < f32::EPSILON && e.translate.is_none() {
        anyhow::bail!("model edit: nothing to do (set --scale and/or --translate, or use --list)");
    }

    let translate = e
        .translate
        .as_deref()
        .map(parse_translate)
        .transpose()?
        .unwrap_or([0.0; 3]);

    let out = e.encode_vpk.context(
        "model edit: provide --encode-vpk <OUT_dir.vpk> to write the edited model (or --list)",
    )?;

    let edit = GeometryEdit {
        part: e.part.clone(),
        scale: e.scale,
        translate,
    };
    let report = edit_model_geometry(
        &e.vpk,
        &e.entry,
        e.base.as_deref(),
        &edit,
        &out,
        e.vpk_entry.as_deref(),
    )
    .with_context(|| format!("editing {} from {}", e.entry, e.vpk.display()))?;

    eprintln!(
        "edited {} buffer(s) across part(s) [{}], {} vertices moved",
        report.edited_buffers,
        report.edited_parts.join(", "),
        report.edited_vertices,
    );
    println!(
        "wrote {}: 1 entry ({}) edited model",
        out.display(),
        report.vpk_entry,
    );
    Ok(())
}

/// `model edit --list`: print the model's vertex buffers (mesh part, block index,
/// vertex count, stride, editability) so a user can pick a `--block`/`--part`.
fn run_model_list_targets(e: &ModelEditArgs) -> Result<()> {
    let targets = model_vertex_targets(&e.vpk, &e.entry, e.base.as_deref())
        .with_context(|| format!("listing targets for {}", e.entry))?;
    println!("{}: {} vertex buffer(s)", e.entry, targets.len());
    for t in &targets {
        println!(
            "  {:<16} block {:<3} {:>7} verts  stride {:<3} {}",
            t.mesh_name,
            t.block_index,
            t.vertex_count,
            t.stride,
            if t.editable {
                "editable"
            } else {
                "not editable"
            },
        );
    }
    Ok(())
}

/// `model edit --list-drawcalls`: print the renderable draw calls (mesh part,
/// material, vertex/index counts) so a user can find a `--remove-material` target.
fn run_model_list_drawcalls(e: &ModelEditArgs) -> Result<()> {
    let calls = model_draw_call_targets(&e.vpk, &e.entry, e.base.as_deref())
        .with_context(|| format!("listing draw calls for {}", e.entry))?;
    println!("{}: {} renderable draw call(s)", e.entry, calls.len());
    for c in &calls {
        println!(
            "  {:<16} {:>7} verts {:>8} idx  {}",
            c.mesh_name, c.vertex_count, c.index_count, c.material
        );
    }
    Ok(())
}

/// `model edit --remove-material`: drop every draw call whose material matches,
/// then pack the edited model into the `--encode-vpk` addon VPK.
fn run_model_remove_material(e: &ModelEditArgs) -> Result<()> {
    let material = e
        .remove_material
        .as_deref()
        .expect("caller checks remove_material is set");
    let out = e.encode_vpk.as_ref().context(
        "model edit --remove-material: provide --encode-vpk <OUT_dir.vpk> for the edited model",
    )?;
    let report = vpkmerge_core::remove_model_material(
        &e.vpk,
        &e.entry,
        e.base.as_deref(),
        material,
        out,
        e.vpk_entry.as_deref(),
    )
    .with_context(|| format!("removing {material:?} from {}", e.entry))?;

    eprintln!("removed {} draw call(s):", report.removed.len());
    for r in &report.removed {
        eprintln!(
            "  {:<16} {:>8} idx  {}",
            r.mesh_name, r.index_count, r.material
        );
    }
    println!(
        "wrote {}: 1 entry ({}) with {} draw call(s) removed",
        out.display(),
        report.vpk_entry,
        report.removed.len(),
    );
    Ok(())
}

/// `model edit --replace-part`: splice a new mesh (from `--from-glb`) over the
/// named part and pack the edited model into the `--encode-vpk` addon VPK.
fn run_model_replace_part(e: &ModelEditArgs, mesh_name: &str) -> Result<()> {
    let in_glb = e
        .from_glb
        .as_ref()
        .context("model edit --replace-part: provide --from-glb <FILE.glb> with the new mesh")?;
    let out = e.encode_vpk.as_ref().context(
        "model edit --replace-part: provide --encode-vpk <OUT_dir.vpk> for the edited model",
    )?;
    let glb_bytes =
        std::fs::read(in_glb).with_context(|| format!("reading {}", in_glb.display()))?;

    let report = vpkmerge_core::replace_model_part(
        &e.vpk,
        &e.entry,
        e.base.as_deref(),
        mesh_name,
        &glb_bytes,
        e.glb_mesh.as_deref(),
        out,
        e.vpk_entry.as_deref(),
    )
    .with_context(|| format!("replacing part {mesh_name:?} in {}", e.entry))?;

    let r = &report.replaced;
    eprintln!(
        "replaced part {:?} ({}): {} -> {} verts, {} -> {} idx (stride {}, idx width {})",
        r.mesh_name,
        r.material,
        r.old_vertex_count,
        r.new_vertex_count,
        r.old_index_count,
        r.new_index_count,
        r.stride,
        r.index_size,
    );
    println!(
        "wrote {}: 1 entry ({}) with part {:?} replaced",
        out.display(),
        report.vpk_entry,
        r.mesh_name,
    );
    Ok(())
}

/// `model edit --reencode-mdat`: diagnostic identity re-encode of every MDAT block
/// (re-emit uncompressed, byte-faithful, no edit), packed to `--encode-vpk`.
fn run_model_reencode_mdat(e: &ModelEditArgs) -> Result<()> {
    let out = e.encode_vpk.as_ref().context(
        "model edit --reencode-mdat: provide --encode-vpk <OUT_dir.vpk> for the re-encoded model",
    )?;
    let count = vpkmerge_core::reencode_model_mdat(
        &e.vpk,
        &e.entry,
        e.base.as_deref(),
        out,
        e.vpk_entry.as_deref(),
    )
    .with_context(|| format!("re-encoding MDAT of {}", e.entry))?;
    println!(
        "wrote {}: {count} MDAT block(s) re-encoded unchanged (identity diagnostic)",
        out.display()
    );
    Ok(())
}

/// Resolves the single vertex buffer (block index) a glb export/import targets,
/// from `--block` (exact) or `--part` (must name a single-buffer editable part).
fn resolve_edit_block(e: &ModelEditArgs) -> Result<usize> {
    let targets = model_vertex_targets(&e.vpk, &e.entry, e.base.as_deref())
        .with_context(|| format!("reading buffers of {}", e.entry))?;

    if let Some(b) = e.block {
        let t = targets
            .iter()
            .find(|t| t.block_index == b)
            .with_context(|| format!("no vertex buffer at block {b} (see --list)"))?;
        if !t.editable {
            anyhow::bail!("block {b} ({}) is not displacement-editable", t.mesh_name);
        }
        return Ok(b);
    }

    let part = e.part.as_deref().context(
        "specify --part <name> or --block <index> to choose the buffer to edit (see --list)",
    )?;
    let hits: Vec<&vpkmerge_core::VertexTarget> = targets
        .iter()
        .filter(|t| t.mesh_name == part && t.editable)
        .collect();
    match hits.len() {
        0 => anyhow::bail!("no editable buffer in part {part:?} (see --list)"),
        1 => Ok(hits[0].block_index),
        _ => {
            let blocks: Vec<String> = hits.iter().map(|t| t.block_index.to_string()).collect();
            anyhow::bail!(
                "part {part:?} has {} buffers (blocks {}); pass --block <index>",
                hits.len(),
                blocks.join(", ")
            )
        }
    }
}

/// Parses a `--translate` value `x,y,z` into a 3-float vector.
fn parse_translate(spec: &str) -> Result<[f32; 3]> {
    let parts: Vec<&str> = spec.split(',').collect();
    if parts.len() != 3 {
        anyhow::bail!("--translate expects x,y,z (three comma-separated numbers), got {spec:?}");
    }
    let mut out = [0.0f32; 3];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p
            .trim()
            .parse()
            .with_context(|| format!("--translate component {:?} is not a number", p.trim()))?;
    }
    Ok(out)
}

/// Recolor one or more models' baked per-vertex colors to a hue and pack them
/// into one addon VPK (`--list` to just see each model's color buffers).
fn run_model_recolor(e: &ModelRecolorArgs) -> Result<()> {
    let base = e.base.as_deref();

    if e.list {
        for entry in &e.entries {
            let targets = model_vertex_targets(&e.vpk, entry, base)
                .with_context(|| format!("listing vertex buffers for {entry}"))?;
            let color: Vec<_> = targets.iter().filter(|t| t.has_color).collect();
            println!("{entry}: {} color-bearing vertex buffer(s)", color.len());
            for t in color {
                println!(
                    "  mesh {:<20} block {:>3}  {} verts",
                    t.mesh_name, t.block_index, t.vertex_count
                );
            }
        }
        return Ok(());
    }

    let out = e.encode_vpk.as_ref().context(
        "model recolor: provide --encode-vpk <OUT_dir.vpk> to write the recolored model(s) \
         (or --list to inspect)",
    )?;

    let recolor = vpkmerge_core::Recolor::new(e.hue, e.saturation, e.brightness);
    let report = vpkmerge_core::recolor_models_to_addon(&e.vpk, &e.entries, base, recolor, out)?;
    let total_verts: usize = report.iter().map(|r| r.stats.vertices).sum();
    for r in &report {
        eprintln!(
            "  {} buffer(s), {} color lane(s), {} verts  {}",
            r.stats.buffers_recolored, r.stats.color_lanes, r.stats.vertices, r.entry
        );
    }
    eprintln!(
        "wrote {}: {} model(s) recolored to hue {} deg ({} verts), each overriding its base entry in place",
        out.display(),
        report.len(),
        e.hue,
        total_verts
    );
    Ok(())
}

fn run_merge(cli: Cli) -> Result<()> {
    let Some(output) = cli.output else {
        anyhow::bail!("missing OUTPUT. Run `vpkmerge --help` for usage.");
    };

    if cli.verbose && !cli.strict {
        let conflicts = vpkmerge_core::detect_conflicts(&cli.inputs)?;
        for c in &conflicts {
            let winner_idx = *c.owner_indices.last().unwrap();
            let winner = cli.inputs[winner_idx].file_name().map_or_else(
                || cli.inputs[winner_idx].display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            println!("override: {} <- {}", c.path, winner);
        }
    }

    let policy = if cli.strict {
        CollisionPolicy::Error
    } else {
        CollisionPolicy::LastWins
    };

    let report = merge(
        &cli.inputs,
        &output,
        &MergeOptions {
            collision_policy: policy,
            ..Default::default()
        },
    )?;

    println!(
        "wrote {}: {} entries, {} paths overridden from {} inputs",
        report.output_path.display(),
        report.total_entries,
        report.overridden_paths,
        report.inputs,
    );
    Ok(())
}

fn run_split(args: SplitCmd) -> Result<()> {
    let plan = read_plan(&args.plan)?;
    let residual_path = args.residual.or(plan.residual);

    let outputs: Vec<SplitOutput> = plan
        .outputs
        .into_iter()
        .map(|o| SplitOutput {
            path: o.path,
            predicate: PathPredicate::AnyPrefix(o.prefixes),
        })
        .collect();

    let policy = if args.strict {
        OverlapPolicy::Error
    } else if args.all_matches {
        OverlapPolicy::AllMatches
    } else {
        OverlapPolicy::FirstMatch
    };

    if args.verbose {
        log_routing(&args.input, &outputs, policy, residual_path.as_deref())?;
    }

    let report = split(
        &args.input,
        &outputs,
        &SplitOptions {
            overlap_policy: policy,
            residual_path,
        },
    )?;

    println!(
        "split {}: {} input entries",
        args.input.display(),
        report.input_entries
    );
    for o in &report.outputs {
        println!("  {:<6} {}  {} entries", "out", o.path.display(), o.entries);
    }
    if let Some(r) = &report.residual {
        println!(
            "  {:<6} {}  {} entries",
            "resid",
            r.path.display(),
            r.entries
        );
    } else if report.unmatched > 0 {
        println!(
            "  {:<6} (dropped, no residual configured)  {} entries",
            "drop", report.unmatched
        );
    }
    Ok(())
}

fn run_portrait(args: PortraitCmd) -> Result<()> {
    let PortraitCmd {
        input,
        out,
        hero,
        manifest,
    } = args;
    let portraits = extract_portraits(&input, hero.as_deref(), &out)?;

    let decoded = portraits.iter().filter(|p| p.output_path.is_some()).count();
    let skipped = portraits.len() - decoded;
    eprintln!(
        "{}: {} portrait{} found, {decoded} decoded, {skipped} skipped",
        input.display(),
        portraits.len(),
        if portraits.len() == 1 { "" } else { "s" },
    );
    for p in &portraits {
        match (&p.output_path, &p.skipped_reason) {
            (Some(out), _) => eprintln!(
                "  {:<13} {:>4}x{:<4} {:<12} -> {}",
                p.hero_codename,
                p.width,
                p.height,
                p.variant.as_str(),
                out.display()
            ),
            (None, Some(reason)) => eprintln!(
                "  {:<13} {:<12} skipped: {reason}",
                p.hero_codename,
                p.variant.as_str()
            ),
            (None, None) => {}
        }
    }

    let manifest_data = Manifest {
        input: input.display().to_string(),
        portraits: portraits.into_iter().map(ManifestEntry::from).collect(),
    };
    let json = serde_json::to_string_pretty(&manifest_data).context("serializing manifest")?;
    if let Some(path) = &manifest {
        std::fs::write(path, &json).with_context(|| format!("writing {}", path.display()))?;
        eprintln!("manifest: {}", path.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn run_soundevents(args: SoundeventsCmd) -> Result<()> {
    let SoundeventsCmd {
        input,
        from_vpk,
        encode,
        encode_vpk,
        vpk_entry,
        swap_vsnd,
        set,
    } = args;

    let label = match &from_vpk {
        Some(vpk) => format!("{} @ {}", input.display(), vpk.display()),
        None => input.display().to_string(),
    };

    let mut se = match &from_vpk {
        Some(vpk) => SoundEvents::from_vpk(vpk, &input.to_string_lossy())?,
        None => SoundEvents::from_file(&input)?,
    };

    // Apply edits (Phase 2). All edits log to stderr.
    for spec in &swap_vsnd {
        let (from, to) = spec
            .split_once('=')
            .with_context(|| format!("--swap-vsnd expects OLD=NEW, got {spec:?}"))?;
        let n = se.swap_vsnd(from, to);
        eprintln!("swap-vsnd: {n} path(s) rewritten {from} -> {to}");
    }
    for spec in &set {
        let (lhs, num) = spec
            .rsplit_once('=')
            .with_context(|| format!("--set expects EVENT/FIELD=NUMBER, got {spec:?}"))?;
        let (event, field) = lhs
            .rsplit_once('/')
            .with_context(|| format!("--set expects EVENT/FIELD=NUMBER, got {spec:?}"))?;
        let value: f64 = num
            .parse()
            .with_context(|| format!("--set value must be a number, got {num:?}"))?;
        if !se.set_event_field(event, field, value) {
            anyhow::bail!("--set: no event named {event:?} in {label}");
        }
        eprintln!("set: {event}/{field} = {value}");
    }

    // Human-readable summary to stderr.
    let summaries = se.summaries();
    eprintln!(
        "{label}: {} event{}",
        summaries.len(),
        if summaries.len() == 1 { "" } else { "s" }
    );
    for s in &summaries {
        let base = s
            .base
            .as_deref()
            .map_or(String::new(), |b| format!("  base={b}"));
        let vol = s.volume.map_or(String::new(), |v| format!("  volume={v}"));
        eprintln!(
            "  {:<34} {} sound{}{base}{vol}",
            s.name,
            s.vsnd_count,
            if s.vsnd_count == 1 { "" } else { "s" }
        );
    }

    // Outputs: a loose file (--encode), a standalone VPK (--encode-vpk), or
    // (if neither) the decoded JSON on stdout. --encode and --encode-vpk can be
    // combined; encode once and reuse the bytes.
    if encode.is_some() || encode_vpk.is_some() {
        let bytes = se.encode()?;
        if let Some(out) = &encode {
            std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
            eprintln!(
                "wrote {}: {} bytes uncompressed (original was {} bytes)",
                out.display(),
                bytes.len(),
                se.original_len()
            );
        }
        if let Some(out) = &encode_vpk {
            let entry = match (&vpk_entry, &from_vpk) {
                (Some(e), _) => e.clone(),
                (None, Some(_)) => input.to_string_lossy().into_owned(),
                (None, None) => anyhow::bail!(
                    "--encode-vpk needs an entry path for a loose-file input: pass \
                     --vpk-entry <PATH> (or read with --from-vpk, which defaults the \
                     entry to INPUT)"
                ),
            };
            vpkmerge_core::pack(&[(entry.as_str(), bytes.as_slice())], out)?;
            eprintln!(
                "wrote {}: 1 entry ({entry}) {} bytes uncompressed",
                out.display(),
                bytes.len(),
            );
        }
    } else {
        let json = serde_json::to_string_pretty(&se.to_json()).context("serializing JSON")?;
        println!("{json}");
    }
    Ok(())
}

/// Recolor every entry in `inputs` (read from `vpk`) to `recolor` and pack the
/// whole set into one addon at `out`, each at its own VPK entry path.
fn recolor_textures_to_addon(
    inputs: &[PathBuf],
    vpk: &Path,
    recolor: vpkmerge_core::Recolor,
    out: &Path,
) -> Result<()> {
    let mut packed: Vec<(String, Vec<u8>)> = Vec::new();
    for input in inputs {
        let entry = input.to_string_lossy().into_owned();
        let bytes = vpkmerge_core::read_vpk_entry(vpk, &entry)?;
        let summary = vpkmerge_core::inspect_texture(&bytes)
            .with_context(|| format!("{entry} is not a readable .vtex_c"))?;
        let recolored = vpkmerge_core::recolor_texture_hue(&bytes, recolor)?;
        eprintln!(
            "  {} {}x{} {}mip -> {} bytes  {entry}",
            summary.format,
            summary.width,
            summary.height,
            summary.mip_count,
            recolored.len()
        );
        packed.push((entry, recolored));
    }
    let refs: Vec<(&str, &[u8])> = packed
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    vpkmerge_core::pack(&refs, out)?;
    eprintln!(
        "wrote {}: {} textures recolored to hue {} deg, each overriding its base entry in place",
        out.display(),
        packed.len(),
        recolor.hue,
    );
    Ok(())
}

fn run_texture(args: TextureCmd) -> Result<()> {
    let TextureCmd {
        inputs,
        from_vpk,
        hue,
        saturation,
        brightness,
        preview,
        encode,
        encode_vpk,
        vpk_entry,
    } = args;

    let recolor = vpkmerge_core::Recolor::new(hue, saturation, brightness);

    // Batch: recolor several entries into one addon (each at its own path).
    if inputs.len() > 1 {
        if preview.is_some() || encode.is_some() || vpk_entry.is_some() {
            anyhow::bail!(
                "--preview/--encode/--vpk-entry apply to a single input; with multiple \
                 INPUTS pass only --encode-vpk <OUT_dir.vpk>"
            );
        }
        let out = encode_vpk.as_ref().context(
            "multiple INPUTS need --encode-vpk <OUT_dir.vpk> to pack them into one addon",
        )?;
        let vpk = from_vpk.as_ref().context(
            "multiple INPUTS must be VPK entry paths: pass --from-vpk <VPK> (each entry packs \
             back at its own path)",
        )?;
        return recolor_textures_to_addon(&inputs, vpk, recolor, out);
    }

    let input = &inputs[0];
    let label = match &from_vpk {
        Some(vpk) => format!("{} @ {}", input.display(), vpk.display()),
        None => input.display().to_string(),
    };

    let bytes = match &from_vpk {
        Some(vpk) => vpkmerge_core::read_vpk_entry(vpk, &input.to_string_lossy())?,
        None => std::fs::read(input).with_context(|| format!("reading {}", input.display()))?,
    };

    let summary = vpkmerge_core::inspect_texture(&bytes)
        .with_context(|| format!("{label} is not a readable .vtex_c"))?;
    eprintln!(
        "{label}: {} {}x{}, {} mip{} -> hue {hue} deg",
        summary.format,
        summary.width,
        summary.height,
        summary.mip_count,
        if summary.mip_count == 1 { "" } else { "s" }
    );

    // Preview is the design-intent color (pre re-encode); cheap, so do it first.
    if let Some(out) = &preview {
        let png = vpkmerge_core::recolor_texture_preview_png(&bytes, recolor)?;
        std::fs::write(out, &png).with_context(|| format!("writing preview {}", out.display()))?;
        eprintln!("wrote preview {} ({} bytes PNG)", out.display(), png.len());
    }

    // Outputs: a loose .vtex_c (--encode), a standalone addon VPK (--encode-vpk),
    // or (if neither, and no --preview) a dry run that just confirms it recolors.
    if encode.is_some() || encode_vpk.is_some() {
        let recolored = vpkmerge_core::recolor_texture_hue(&bytes, recolor)?;
        if let Some(out) = &encode {
            std::fs::write(out, &recolored)
                .with_context(|| format!("writing {}", out.display()))?;
            eprintln!(
                "wrote {}: {} bytes (recolored .vtex_c)",
                out.display(),
                recolored.len()
            );
        }
        if let Some(out) = &encode_vpk {
            let entry = match (&vpk_entry, &from_vpk) {
                (Some(e), _) => e.clone(),
                (None, Some(_)) => input.to_string_lossy().into_owned(),
                (None, None) => anyhow::bail!(
                    "--encode-vpk needs an entry path for a loose-file input: pass \
                     --vpk-entry <PATH> (or read with --from-vpk, which defaults the \
                     entry to INPUT)"
                ),
            };
            vpkmerge_core::pack(&[(entry.as_str(), recolored.as_slice())], out)?;
            eprintln!(
                "wrote {}: 1 entry ({entry}) overrides the base texture in place",
                out.display()
            );
        }
    } else if preview.is_none() {
        // Dry run: prove the recolor path works end to end without writing.
        vpkmerge_core::recolor_texture_hue(&bytes, recolor)?;
        eprintln!(
            "dry run OK (recolor succeeds); pass --encode <FILE>, --encode-vpk <OUT_dir.vpk>, \
             or --preview <PNG> to write output"
        );
    }
    Ok(())
}

/// Recolor a hero's full ability VFX to one target and pack it into a single
/// addon (or, with `--preview-png`, just render a fast swatch and exit).
fn run_recolor_hero(args: &RecolorHeroCmd) -> Result<()> {
    let recolor = vpkmerge_core::Recolor::new(args.hue, args.saturation, args.brightness);

    // Fast path: render the recipe's representative texture as a PNG swatch (no
    // bake, no re-encode), for a live UI preview.
    if let Some(png_out) = &args.preview_png {
        let png = vpkmerge_core::recolor_hero_preview_png(
            &args.vpk,
            args.base.as_deref(),
            &args.hero,
            recolor,
        )
        .with_context(|| format!("previewing hero {} recolor", args.hero))?;
        std::fs::write(png_out, &png)
            .with_context(|| format!("writing preview {}", png_out.display()))?;
        eprintln!(
            "wrote preview {} ({} bytes PNG) for hero {} at hue {} sat {} val {}",
            png_out.display(),
            png.len(),
            args.hero,
            args.hue,
            args.saturation,
            args.brightness,
        );
        return Ok(());
    }

    let out = args.encode_vpk.as_ref().context(
        "recolor-hero: provide --encode-vpk <OUT_dir.vpk> to bake the addon (or --preview-png \
         <PNG> for a fast swatch)",
    )?;
    let report = vpkmerge_core::recolor_hero_to_addon(
        &args.vpk,
        args.base.as_deref(),
        &args.hero,
        recolor,
        out,
    )
    .with_context(|| format!("recoloring hero {} to hue {}", args.hero, args.hue))?;

    eprintln!(
        "{}: {} particle(s) recolored ({} color-free skipped), {} texture(s), {} model(s) ({} verts)",
        report.codename,
        report.particles_recolored,
        report.particles_no_color,
        report.textures_recolored,
        report.models_recolored,
        report.model_vertices,
    );
    println!(
        "wrote {}: {} entries, hero {} recolored to hue {} deg (overrides the base in place)",
        out.display(),
        report.total_entries,
        report.codename,
        args.hue,
    );
    Ok(())
}

fn read_plan(path: &Path) -> Result<PlanFile> {
    let bytes = std::fs::read(path).with_context(|| format!("reading plan {}", path.display()))?;
    let plan: PlanFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing plan {}", path.display()))?;
    Ok(plan)
}

fn log_routing(
    input: &Path,
    outputs: &[SplitOutput],
    policy: OverlapPolicy,
    residual: Option<&Path>,
) -> Result<()> {
    let info = vpkmerge_core::inspect(input)?;
    for path in &info.file_paths {
        let mut hits: Vec<&Path> = Vec::new();
        for o in outputs {
            if predicate_matches(&o.predicate, path) {
                hits.push(&o.path);
                if policy == OverlapPolicy::FirstMatch {
                    break;
                }
            }
        }
        if hits.is_empty() {
            if let Some(r) = residual {
                println!("residual: {path}  ->  {}", r.display());
            } else {
                println!("dropped : {path}");
            }
        } else {
            for h in hits {
                println!("route   : {path}  ->  {}", h.display());
            }
        }
    }
    Ok(())
}

fn predicate_matches(pred: &PathPredicate, path: &str) -> bool {
    match pred {
        PathPredicate::AnyPrefix(prefixes) => prefixes.iter().any(|p| path.starts_with(p)),
    }
}
