mod cli;
mod db;
mod error;
mod export;
mod hash;
mod identity;
mod scanner;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Index(args) => cli::cmd_index(args),
        Commands::Update(args) => cli::cmd_update(args),
        Commands::Dup(args) => cli::cmd_dup(args),
        Commands::Export(args) => cli::cmd_export(args),
        Commands::Stats(args) => cli::cmd_stats(args),
    }
}
