use std::path::{Path, PathBuf};

use clap::Parser;
use marauders::api;

#[derive(Parser, Debug)]
#[command(
    name = "marauders-import-rust-mutants",
    about = "Import external Rust mutants into Marauders comment syntax"
)]
struct Opts {
    #[arg(long)]
    base: PathBuf,
    #[arg(long)]
    mutant: Vec<PathBuf>,
    #[arg(long)]
    mutants_dir: Option<PathBuf>,
    #[arg(long)]
    cargo_mutants_dir: Option<PathBuf>,
    #[arg(long, default_value = "ext_mut")]
    prefix: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    diffs: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let opts = Opts::parse();
    run_import_rust_mutants_command(
        &opts.base,
        &opts.mutant,
        opts.mutants_dir.as_deref(),
        opts.cargo_mutants_dir.as_deref(),
        &opts.prefix,
        opts.output.as_deref(),
        opts.diffs,
    )
}

fn run_import_rust_mutants_command(
    base: &Path,
    mutants: &[PathBuf],
    mutants_dir: Option<&Path>,
    cargo_mutants_dir: Option<&Path>,
    prefix: &str,
    output: Option<&Path>,
    diffs: bool,
) -> anyhow::Result<()> {
    if let Some(dir) = cargo_mutants_dir {
        let converted =
            api::import_rust_mutants_from_cargo_mutants_output(base, dir, output, prefix)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        log::info!(
            "imported cargo-mutants output into '{}'",
            converted.to_string_lossy()
        );
        return Ok(());
    }

    if mutants_dir.is_none() && mutants.is_empty() {
        let destination = output.unwrap_or(base);
        let converted =
            api::auto_generate_and_import_rust_mutants(base, destination, prefix, diffs)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        log::info!(
            "auto-generated cargo-mutants output and imported into '{}'",
            converted.to_string_lossy()
        );
        return Ok(());
    }

    let resolved_mutants = if let Some(dir) = mutants_dir {
        api::collect_rust_mutants_from_dir(base, dir).map_err(|e| anyhow::anyhow!("{}", e))?
    } else {
        mutants.to_vec()
    };
    if resolved_mutants.is_empty() {
        anyhow::bail!(
            "provide --mutant paths, --mutants-dir, --cargo-mutants-dir, or only --base to auto-run cargo-mutants"
        );
    }

    let converted = api::import_rust_mutants(base, &resolved_mutants, output, prefix)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    log::info!(
        "imported {} mutant file(s) into '{}'",
        resolved_mutants.len(),
        converted.to_string_lossy()
    );
    Ok(())
}
