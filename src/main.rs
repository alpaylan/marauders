
use clap::Parser;
use cli::Opts;

mod algebra;
mod cli;
mod code;
mod languages;
mod syntax;
mod variation;

fn main() -> Result<(), Box<dyn std::error::Error>>{

    env_logger::init();

    let opts = Opts::parse();

    cli::run(opts)?;
    
    Ok(())
}
