mod abi;
mod cli;
mod commands;
mod config;
mod rpc;
mod types;

use anyhow::Result;
use clap::Parser;

use tracing_subscriber::{fmt, EnvFilter};

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_target(true) // show module path
        .with_thread_ids(true) // useful for async
        .with_line_number(true)
        .compact() // or .pretty()
        .init();
}
#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    tracing::info!("logger initialized");
    tracing::debug!("debug logging enabled");

    let cli = cli::Cli::parse();
    let config = config::Config::load(cli.config_path.as_deref())?;
    cli.run(config).await
}
