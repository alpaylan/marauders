
use clap::Parser;

use crate::code::{Code, SpanContent};

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
        path: String,
    },
    #[clap(name = "set", about = "Set active variant")]
    Set {
        #[clap(short, long)]
        path: String,
        #[clap(short, long)]
        variant: String,
    },
    #[clap(name = "unset", about = "Unset active variant")]
    Unset {
        #[clap(short, long)]
        path: String,
        #[clap(short, long)]
        variant: String,
    },
}

pub(crate) fn run(opts: Opts) -> anyhow::Result<()> {
    match &opts.command {
        Command::List { path } => {
            log::info!("listing variations at '{}'", path);
            run_list_command(path)?;
        }
        Command::Set { path, variant } => {
            log::info!("set active variant '{}' at '{}'", variant, path);
            run_set_command(path, variant)?;
        }
        Command::Unset { path, variant } => {
            log::info!("unset active variant '{}'", variant);
            run_unset_command(path, variant)?;
        }
    }

    Ok(())
}

fn run_list_command(path: &str) -> anyhow::Result<()> {
    // todo: handle directories and recursive listing
    let code = &mut Code::from_file(path)?;

    for span in code.parts.iter() {
        match &span.content {
            SpanContent::Variation(v) => {
                println!("{}:{} {}", path, span.line, v);
            }
            _ => {}
        }
    }
    Ok(())
}

fn run_set_command(path: &str, variant: &str) -> anyhow::Result<()> {
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
        .ok_or_else(|| anyhow::anyhow!("variant not found"))?;
    
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

    code.set_active_variant(variation_index, variant_index)
}


fn run_unset_command(path: &str, variant: &str) -> anyhow::Result<()> {
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
