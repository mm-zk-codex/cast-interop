use crate::commands;
use crate::config::Config;
use crate::types::AddressBook;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "cast-interop",
    version,
    about = "Interop-focused cast-like CLI for zkSync"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub config_path: Option<PathBuf>,

    #[arg(long, global = true)]
    pub center: Option<String>,

    #[arg(long, global = true)]
    pub handler: Option<String>,

    #[arg(long, global = true)]
    pub root_storage: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub async fn run(self, config: Config) -> Result<()> {
        let addresses = AddressBook::from_config_and_flags(
            &config,
            self.center.as_deref(),
            self.handler.as_deref(),
            self.root_storage.as_deref(),
        )?;

        match self.command {
            Command::Tx(cmd) => cmd.run(config, addresses).await,
            Command::Bundle(cmd) => cmd.run(config, addresses).await,
            Command::Proof(cmd) => cmd.run(config, addresses).await,
            Command::Root(cmd) => cmd.run(config, addresses).await,
            Command::Status(cmd) => cmd.run(config, addresses).await,
            Command::Relay(cmd) => cmd.run(config, addresses).await,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Tx(TxCommand),
    Bundle(BundleCommand),
    Proof(ProofCommand),
    Root(RootCommand),
    Status(StatusCommand),
    Relay(RelayCommand),
}

#[derive(Parser, Debug)]
pub struct TxCommand {
    #[command(subcommand)]
    pub command: TxSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum TxSubcommand {
    Show(TxShowArgs),
}

impl TxCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            TxSubcommand::Show(args) => commands::tx_show::run(args, config, addresses).await,
        }
    }
}

#[derive(Parser, Debug)]
pub struct BundleCommand {
    #[command(subcommand)]
    pub command: BundleSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum BundleSubcommand {
    Extract(BundleExtractArgs),
    Verify(BundleActionArgs),
    Execute(BundleActionArgs),
}

impl BundleCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            BundleSubcommand::Extract(args) => {
                commands::bundle_extract::run(args, config, addresses).await
            }
            BundleSubcommand::Verify(args) => {
                commands::bundle_action::run_verify(args, config, addresses).await
            }
            BundleSubcommand::Execute(args) => {
                commands::bundle_action::run_execute(args, config, addresses).await
            }
        }
    }
}

#[derive(Parser, Debug)]
pub struct ProofCommand {
    #[command(flatten)]
    pub args: ProofArgs,
}

impl ProofCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::proof::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct RootCommand {
    #[command(subcommand)]
    pub command: RootSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum RootSubcommand {
    Wait(RootWaitArgs),
}

impl RootCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            RootSubcommand::Wait(args) => commands::root_wait::run(args, config, addresses).await,
        }
    }
}

#[derive(Parser, Debug)]
pub struct StatusCommand {
    #[command(flatten)]
    pub args: StatusArgs,
}

impl StatusCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::status::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct RelayCommand {
    #[command(flatten)]
    pub args: RelayArgs,
}

impl RelayCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        todo!();
        //commands::relay::run(self.args, config, addresses).await
    }
}

#[derive(Args, Debug)]
pub struct TxShowArgs {
    #[arg(long)]
    pub rpc: String,

    pub tx_hash: String,

    #[arg(long)]
    pub interop_only: bool,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct BundleExtractArgs {
    #[arg(long)]
    pub rpc: String,

    #[arg(long)]
    pub tx: String,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long)]
    pub json_out: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ProofArgs {
    #[arg(long)]
    pub rpc: String,

    #[arg(long)]
    pub tx: String,

    #[arg(long, default_value_t = 0)]
    pub msg_index: u32,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long)]
    pub no_wait: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long)]
    pub timeout_ms: Option<u64>,

    #[arg(long)]
    pub poll_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub struct RootWaitArgs {
    #[arg(long)]
    pub rpc: String,

    #[arg(long)]
    pub source_chain: String,

    #[arg(long)]
    pub batch: u64,

    #[arg(long)]
    pub expected_root: String,

    #[arg(long)]
    pub timeout_ms: Option<u64>,

    #[arg(long)]
    pub poll_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub struct BundleActionArgs {
    #[arg(long)]
    pub rpc: String,

    #[arg(long)]
    pub bundle: String,

    #[arg(long)]
    pub proof: String,

    #[arg(long)]
    pub handler: Option<String>,

    #[arg(long)]
    pub center: Option<String>,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub private_key: Option<String>,

    #[arg(long)]
    pub private_key_env: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[arg(long)]
    pub rpc: String,

    #[arg(long)]
    pub bundle_hash: String,

    #[arg(long)]
    pub handler: Option<String>,

    #[arg(long)]
    pub bundle: Option<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct RelayArgs {
    #[arg(long)]
    pub rpc_src: String,

    #[arg(long)]
    pub rpc_dest: String,

    #[arg(long)]
    pub tx: String,

    #[arg(long, default_value_t = 0)]
    pub msg_index: u32,

    #[arg(long, default_value = "execute")]
    pub mode: String,

    #[arg(long)]
    pub out_dir: Option<PathBuf>,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub private_key: Option<String>,

    #[arg(long)]
    pub private_key_env: Option<String>,

    #[arg(long)]
    pub handler: Option<String>,

    #[arg(long)]
    pub center: Option<String>,

    #[arg(long)]
    pub root_storage: Option<String>,

    #[arg(long)]
    pub timeout_ms: Option<u64>,

    #[arg(long)]
    pub poll_ms: Option<u64>,
}
