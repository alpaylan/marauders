use clap::Parser;
use cli::Opts;

mod algebra;
mod cli;
mod code;
mod languages;
mod project;
mod syntax;
mod variation;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let opts = Opts::parse();

    cli::run(opts)
}
