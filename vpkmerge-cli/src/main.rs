use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};
use vpkmerge_core::{
    merge, split, CollisionPolicy, MergeOptions, OverlapPolicy, PathPredicate, SplitOptions,
    SplitOutput,
};

#[derive(Parser)]
#[command(
    name = "vpkmerge",
    version,
    about = "Combine multiple VPK files into one (or split one into many).",
    long_about = "Combine multiple Valve Pak (VPK) files into one. Built for Deadlock \
                  modding so players can pre-merge mods and bypass the ~100-mod mount limit.\n\
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Some(Command::Split(args)) => run_split(args),
        None => run_merge(cli),
    }
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
