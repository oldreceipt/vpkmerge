use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use vpkmerge_core::{merge, CollisionPolicy, MergeOptions};

#[derive(Parser)]
#[command(
    name = "vpkmerge",
    version,
    about = "Combine multiple VPK files into one",
    long_about = "Combine multiple Valve Pak (VPK) files into one. Built for Deadlock \
                  modding so players can pre-merge mods and bypass the ~100-mod mount limit.\n\
                  \n\
                  By default, later inputs win on path collision. Use --strict to refuse to \
                  merge when paths collide."
)]
struct Cli {
    /// Output VPK path. Parent directory is created if missing.
    output: PathBuf,

    /// Input VPK paths. At least 2 required.
    #[arg(required = true, num_args = 2..)]
    inputs: Vec<PathBuf>,

    /// Error out on any path collision instead of letting later inputs win.
    #[arg(long)]
    strict: bool,

    /// Print each path that gets overridden by a later input.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
        &cli.output,
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
