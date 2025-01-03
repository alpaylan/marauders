use std::path::{Path, PathBuf};

use clap::Parser;

use crate::{
    code::{Code, SpanContent},
    project::Project,
};

#[derive(Parser)]
pub(crate) struct Opts {
    #[command(subcommand)]
    command: Command,
}

#[derive(Parser)]
pub(crate) enum Command {
    #[clap(name = "list", about = "List variations in the code")]
    List {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(long)]
        pattern: Option<String>,
    },
    #[clap(name = "set", about = "Set active variant")]
    Set {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "unset", about = "Unset active variant")]
    Unset {
        #[clap(short, long)]
        path: PathBuf,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "reset", about = "Reset all variationts to base")]
    Reset {
        #[clap(short, long)]
        path: PathBuf,
    },
}

pub(crate) fn run(opts: Opts) -> anyhow::Result<()> {
    match &opts.command {
        Command::List { path, pattern } => {
            log::info!("listing variations at '{}'", path.to_string_lossy());
            run_list_command(path, pattern.as_deref())?;
        }
        Command::Set { path, variant } => {
            log::info!(
                "set active variant '{}' at '{}'",
                variant,
                path.to_string_lossy()
            );
            run_set_command(path, variant)?;
        }
        Command::Unset { path, variant } => {
            log::info!("unset active variant '{}'", variant);
            run_unset_command(path, variant)?;
        }
        Command::Reset { path } => {
            log::info!(
                "resetting all variations to base at '{}'",
                path.to_string_lossy()
            );
            run_reset_command(path)?;
        }
    }

    Ok(())
}

fn run_list_command(path: &Path, pattern: Option<&str>) -> anyhow::Result<()> {
    let project = Project::new(path, pattern);

    match project {
        Ok(project) => {
            for file in project.files.iter() {
                let code = &file.code;

                for span in code.parts.iter() {
                    if let SpanContent::Variation(v) = &span.content {
                        println!("{}:{} {}", file.path.to_string_lossy(), span.line, v);
                    }
                }
            }
        }
        // todo: change this to a more descriptive sum type instead of an error
        Err(_) => {
            let code = &mut Code::from_file(path)?;

            for span in code.parts.iter() {
                if let SpanContent::Variation(v) = &span.content {
                    println!("{}:{} {}", path.to_string_lossy(), span.line, v);
                }
            }
        }
    }

    Ok(())
}

fn run_set_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, and do not set it again
    let code = &mut Code::from_file(path)?;

    let (variation_index, variation) = code
        .parts
        .iter()
        .enumerate()
        .find(|(_, v)| match &v.content {
            SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
            _ => false,
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "variant '{variant}' not found, possible variants are ({})",
                code.get_all_variants().join(",")
            )
        })?;

    let variation = match &variation.content {
        SpanContent::Variation(v) => v,
        _ => unreachable!(),
    };

    let (variant_index, _) = variation
        .variants
        .iter()
        .enumerate()
        .find(|(_, v)| v.name == variant)
        .ok_or_else(|| anyhow::anyhow!("variant not found"))?;

    // Shift index by because 0 is reserved for the base code
    let variant_index = variant_index + 1;

    log::info!(
        "variant index is '{}' at '({}, {})'",
        variant_index,
        variation.name.as_deref().unwrap_or("anonymous"),
        variation_index,
    );

    code.set_active_variant(variation_index, variant_index)?;

    log::info!("active variant set to '{}'", variant);
    println!("active variant set to '{}'", variant);

    Ok(())
}

fn run_unset_command(path: &Path, variant: &str) -> anyhow::Result<()> {
    // todo: check currently active variant, if it is not set, do not unset it

    let code = &mut Code::from_file(path)?;

    let (variation_index, variation) = code
        .parts
        .iter()
        .enumerate()
        .find(|(_, v)| match &v.content {
            SpanContent::Variation(v) => v.variants.iter().any(|v| v.name == variant),
            _ => false,
        })
        .ok_or_else(|| anyhow::anyhow!("variant not found"))?;

    let variation = match &variation.content {
        SpanContent::Variation(v) => v,
        _ => unreachable!(),
    };

    log::info!(
        "variation is '({}, {})'",
        variation.name.as_deref().unwrap_or("anonymous"),
        variation_index,
    );
    // todo: this is a bug, if the user unsets any variant in a variation, the whole variation gets unset, not the variant
    code.set_active_variant(variation_index, 0)
}

fn run_reset_command(path: &Path) -> anyhow::Result<()> {
    let code = &mut Code::from_file(path)?;

    code.parts.iter_mut().for_each(|span| {
        if let SpanContent::Variation(v) = &mut span.content {
            v.active = 0;
        }
    });

    code.save_to_file(path)?;

    log::info!("all variations reset to base");
    println!("all variations reset to base");

    Ok(())
}
