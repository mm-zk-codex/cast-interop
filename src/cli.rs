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
            Command::Chains(cmd) => cmd.run(config, addresses).await,
            Command::Rpc(cmd) => cmd.run(config, addresses).await,
            Command::Contracts(cmd) => cmd.run(config, addresses).await,
            Command::Send(cmd) => cmd.run(config, addresses).await,
            Command::Token(cmd) => cmd.run(config, addresses).await,
            Command::Encode(cmd) => cmd.run(config, addresses).await,
            Command::Watch(cmd) => cmd.run(config, addresses).await,
            Command::Doctor(cmd) => cmd.run(config, addresses).await,
            Command::Explain(cmd) => cmd.run(config, addresses).await,
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
    Chains(ChainsCommand),
    Rpc(RpcCommand),
    Contracts(ContractsCommand),
    Send(SendCommand),
    Token(TokenCommand),
    Encode(EncodeCommand),
    Watch(WatchCommand),
    Doctor(DoctorCommand),
    Explain(ExplainCommand),
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
        commands::relay::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct ChainsCommand {
    #[command(subcommand)]
    pub command: ChainsSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum ChainsSubcommand {
    List(ChainsListArgs),
    Add(ChainsAddArgs),
    Rm(ChainsRemoveArgs),
}

impl ChainsCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            ChainsSubcommand::List(args) => {
                commands::chains::run_list(args, config, addresses).await
            }
            ChainsSubcommand::Add(args) => commands::chains::run_add(args, config, addresses).await,
            ChainsSubcommand::Rm(args) => {
                commands::chains::run_remove(args, config, addresses).await
            }
        }
    }
}

#[derive(Parser, Debug)]
pub struct RpcCommand {
    #[command(subcommand)]
    pub command: RpcSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum RpcSubcommand {
    Ping(RpcPingArgs),
}

impl RpcCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            RpcSubcommand::Ping(args) => commands::rpc_ping::run(args, config, addresses).await,
        }
    }
}

#[derive(Parser, Debug)]
pub struct ContractsCommand {
    #[command(flatten)]
    pub args: ContractsArgs,
}

impl ContractsCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::contracts::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct SendCommand {
    #[command(subcommand)]
    pub command: SendSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum SendSubcommand {
    Message(SendMessageArgs),
    Bundle(SendBundleArgs),
}

impl SendCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            SendSubcommand::Message(args) => {
                commands::send::run_message(args, config, addresses).await
            }
            SendSubcommand::Bundle(args) => {
                commands::send::run_bundle(args, config, addresses).await
            }
        }
    }
}

#[derive(Parser, Debug)]
pub struct TokenCommand {
    #[command(subcommand)]
    pub command: TokenSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum TokenSubcommand {
    #[command(name = "wrap-info")]
    WrapInfo(TokenWrapInfoArgs),
    Status(TokenStatusArgs),
    Send(TokenSendArgs),
}

impl TokenCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            TokenSubcommand::WrapInfo(args) => {
                commands::token::run_wrap_info(args, config, addresses).await
            }
            TokenSubcommand::Status(args) => {
                commands::token::run_status(args, config, addresses).await
            }
            TokenSubcommand::Send(args) => commands::token::run_send(args, config, addresses).await,
        }
    }
}

#[derive(Parser, Debug)]
pub struct EncodeCommand {
    #[command(subcommand)]
    pub command: EncodeSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum EncodeSubcommand {
    #[command(name = "7930")]
    Erc7930(Encode7930Args),
    Attrs(EncodeAttrsArgs),
    #[command(name = "asset-id")]
    AssetId(EncodeAssetIdArgs),
}

impl EncodeCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            EncodeSubcommand::Erc7930(args) => {
                commands::encode::run_7930(args, config, addresses).await
            }
            EncodeSubcommand::Attrs(args) => {
                commands::encode::run_attrs(args, config, addresses).await
            }
            EncodeSubcommand::AssetId(args) => {
                commands::encode::run_asset_id(args, config, addresses).await
            }
        }
    }
}

#[derive(Parser, Debug)]
pub struct WatchCommand {
    #[command(flatten)]
    pub args: WatchArgs,
}

impl WatchCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::watch::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct DoctorCommand {
    #[command(flatten)]
    pub args: DoctorArgs,
}

impl DoctorCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::doctor::run(self.args, config, addresses).await
    }
}

#[derive(Parser, Debug)]
pub struct ExplainCommand {
    #[command(flatten)]
    pub args: ExplainArgs,
}

impl ExplainCommand {
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        commands::explain::run(self.args, config, addresses).await
    }
}

#[derive(Args, Debug, Clone)]
pub struct RpcSelectionArgs {
    #[arg(long)]
    pub rpc: Option<String>,

    #[arg(long)]
    pub chain: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct SignerArgs {
    #[arg(long)]
    pub private_key: Option<String>,

    #[arg(long)]
    pub private_key_env: Option<String>,
}

#[derive(Args, Debug)]
pub struct TxShowArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    pub tx_hash: String,

    #[arg(long)]
    pub interop_only: bool,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct BundleExtractArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub tx: String,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long)]
    pub json_out: Option<PathBuf>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ProofArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

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
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

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
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

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

    #[command(flatten)]
    pub signer: SignerArgs,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

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
    pub rpc_src: Option<String>,

    #[arg(long)]
    pub chain_src: Option<String>,

    #[arg(long)]
    pub rpc_dest: Option<String>,

    #[arg(long)]
    pub chain_dest: Option<String>,

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

    #[command(flatten)]
    pub signer: SignerArgs,

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

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ChainsListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ChainsAddArgs {
    pub alias: String,

    #[arg(long)]
    pub rpc: String,
}

#[derive(Args, Debug)]
pub struct ChainsRemoveArgs {
    pub alias: String,
}

#[derive(Args, Debug)]
pub struct RpcPingArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ContractsArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct SendMessageArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub to_chain: String,

    #[arg(long)]
    pub to: String,

    #[arg(long)]
    pub payload: Option<String>,

    #[arg(long)]
    pub payload_file: Option<PathBuf>,

    #[arg(long)]
    pub interop_value: Option<String>,

    #[arg(long)]
    pub indirect: Option<String>,

    #[arg(long)]
    pub execution_address: Option<String>,

    #[arg(long)]
    pub unbundler: Option<String>,

    #[arg(long)]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct SendBundleArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub to_chain: String,

    #[arg(long)]
    pub calls: PathBuf,

    #[arg(long)]
    pub bundle_execution_address: Option<String>,

    #[arg(long)]
    pub bundle_unbundler: Option<String>,

    #[arg(long)]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct TokenWrapInfoArgs {
    #[arg(long)]
    pub rpc_src: Option<String>,

    #[arg(long)]
    pub chain_src: Option<String>,

    #[arg(long)]
    pub rpc_dest: Option<String>,

    #[arg(long)]
    pub chain_dest: Option<String>,

    #[arg(long)]
    pub token: String,

    #[arg(long)]
    pub native_token_vault: Option<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct TokenStatusArgs {
    #[arg(long)]
    pub rpc_src: Option<String>,

    #[arg(long)]
    pub chain_src: Option<String>,

    #[arg(long)]
    pub rpc_dest: Option<String>,

    #[arg(long)]
    pub chain_dest: Option<String>,

    #[arg(long)]
    pub token: String,

    #[arg(long)]
    pub to: String,

    #[arg(long)]
    pub native_token_vault: Option<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct TokenSendArgs {
    #[arg(long)]
    pub rpc_src: Option<String>,

    #[arg(long)]
    pub chain_src: Option<String>,

    #[arg(long)]
    pub rpc_dest: Option<String>,

    #[arg(long)]
    pub chain_dest: Option<String>,

    #[arg(long)]
    pub token: String,

    #[arg(long)]
    pub amount: Option<String>,

    #[arg(long)]
    pub amount_wei: Option<String>,

    #[arg(long)]
    pub decimals: Option<u32>,

    #[arg(long)]
    pub to: String,

    #[arg(long, default_value = "0")]
    pub indirect_msg_value: String,

    #[arg(long)]
    pub interop_value: Option<String>,

    #[arg(long)]
    pub unbundler: Option<String>,

    #[arg(long)]
    pub asset_router: Option<String>,

    #[arg(long)]
    pub native_token_vault: Option<String>,

    #[arg(long)]
    pub skip_register: bool,

    #[arg(long)]
    pub skip_approve: bool,

    #[arg(long)]
    pub approve_amount: Option<String>,

    #[arg(long, default_value = "execute")]
    pub mode: String,

    #[arg(long)]
    pub watch: bool,

    #[arg(long)]
    pub timeout_ms: Option<u64>,

    #[arg(long)]
    pub poll_ms: Option<u64>,

    #[arg(long)]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,
}

#[derive(Args, Debug)]
pub struct Encode7930Args {
    #[arg(long)]
    pub chain_id: Option<String>,

    #[arg(long)]
    pub address: Option<String>,

    #[arg(long)]
    pub address_only: Option<String>,
}

#[derive(Args, Debug)]
pub struct EncodeAttrsArgs {
    #[arg(long)]
    pub interop_value: Option<String>,

    #[arg(long)]
    pub indirect: Option<String>,

    #[arg(long)]
    pub execution_address: Option<String>,

    #[arg(long)]
    pub unbundler: Option<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct EncodeAssetIdArgs {
    #[arg(long)]
    pub chain_id: String,

    #[arg(long)]
    pub token: String,

    #[arg(long)]
    pub native_token_vault: Option<String>,
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    #[arg(long)]
    pub rpc_src: Option<String>,

    #[arg(long)]
    pub chain_src: Option<String>,

    #[arg(long)]
    pub rpc_dest: Option<String>,

    #[arg(long)]
    pub chain_dest: Option<String>,

    #[arg(long)]
    pub tx: String,

    #[arg(long, default_value_t = 0)]
    pub msg_index: u32,

    #[arg(long)]
    pub until: Option<String>,

    #[arg(long)]
    pub poll_ms: Option<u64>,

    #[arg(long)]
    pub timeout_ms: Option<u64>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ExplainArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long)]
    pub bundle: String,

    #[arg(long)]
    pub proof: String,

    #[arg(long)]
    pub handler: Option<String>,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long)]
    pub json: bool,
}
