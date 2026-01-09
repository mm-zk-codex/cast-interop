mod abi;
mod cli;
mod commands;
mod config;
mod rpc;
mod types;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let config = config::Config::load(cli.config_path.as_deref())?;
    cli.run(config).await
}
