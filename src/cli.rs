use crate::commands;
use crate::config::Config;
use crate::types::AddressBook;
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Entry point for the cast-interop CLI.
///
/// Use this tool to build, relay, and debug zkSync interop bundles and token
/// transfers without wiring RPC/ABI details every time.
#[derive(Parser, Debug)]
#[command(
    name = "cast-interop",
    version,
    about = "Interop-focused cast-like CLI for zkSync",
    long_about = "Interop-focused cast-like CLI for zkSync.\nUse it to send tokens, build bundles, and debug interop flows across chains.\nExample: cast-interop token send --chain-src era --chain-dest test --token 0xTOKEN --amount 1 --to 0xRECIPIENT --private-key $PRIVATE_KEY"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Path to the config file. Default: ~/.config/cast-interop/config.toml."
    )]
    pub config_path: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "ADDRESS",
        help = "Override the interop center contract address. Default: config addresses.interop_center."
    )]
    pub center: Option<String>,

    #[arg(
        long,
        global = true,
        value_name = "ADDRESS",
        help = "Override the interop handler contract address. Default: config addresses.interop_handler."
    )]
    pub handler: Option<String>,

    #[arg(
        long,
        global = true,
        value_name = "ADDRESS",
        help = "Override the interop root storage contract address. Default: config addresses.interop_root_storage."
    )]
    pub root_storage: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Dispatch the selected command.
    pub async fn run(self, config: Config) -> Result<()> {
        let addresses = AddressBook::from_config_and_flags(
            &config,
            self.center.as_deref(),
            self.handler.as_deref(),
            self.root_storage.as_deref(),
        )?;

        match self.command {
            Command::Token(cmd) => cmd.run(config, addresses).await,
            Command::Bundle(cmd) => cmd.run(config, addresses).await,
            Command::Send(cmd) => cmd.run(config, addresses).await,
            Command::Debug(cmd) => cmd.run(config, addresses).await,
            Command::Encode(cmd) => cmd.run(config, addresses).await,
            Command::Chains(cmd) => cmd.run(config, addresses).await,
        }
    }
}

/// Top-level command groups for interop workflows.
#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(
        about = "Token bridging workflows.",
        long_about = "Inspect wrapped tokens, check balances, or send ERC20s across chains.\nUse this when bridging assets through interop.\nExample: cast-interop token send --chain-src era --chain-dest test --token 0xTOKEN --amount 1 --to 0xRECIPIENT --private-key $PRIVATE_KEY"
    )]
    Token(TokenCommand),
    #[command(
        about = "Bundle lifecycle workflows.",
        long_about = "Extract, verify, execute, relay, and explain interop bundles.\nUse this to move bundles across chains or inspect their status.\nExample: cast-interop bundle relay --chain-src era --chain-dest test --tx 0xTX --private-key $PRIVATE_KEY"
    )]
    Bundle(BundleCommand),
    #[command(
        about = "Send interop messages or bundles.",
        long_about = "Create and send interop messages or bundles from a source chain.\nUse this when you want to author a new interop payload.\nExample: cast-interop send message --chain era --to-chain test --to 0xTARGET --payload 0xdeadbeef --private-key $PRIVATE_KEY"
    )]
    Send(SendCommand),
    #[command(
        about = "Debug and observability commands.",
        long_about = "Inspect transactions, proofs, roots, RPC capabilities, and watches.\nUse these when a relay or token transfer is stuck.\nExample: cast-interop debug tx --chain era 0xTX"
    )]
    Debug(DebugCommand),
    #[command(
        about = "Encoding utilities.",
        long_about = "Encode ERC-7930 addresses, call attributes, or asset IDs.\nUse these for low-level data preparation and inspection.\nExample: cast-interop encode asset-id --chain-id 324 --token 0xTOKEN"
    )]
    Encode(EncodeCommand),
    #[command(
        about = "Manage configured chains.",
        long_about = "Add, list, or remove chain aliases in the config file.\nUse this to avoid repeating RPC URLs.\nExample: cast-interop chains add era --rpc https://mainnet.era.zksync.io"
    )]
    Chains(ChainsCommand),
}

/// Debug and observability helpers.
#[derive(Parser, Debug)]
#[command(
    about = "Debug and observability commands.",
    long_about = "Inspect transactions, proofs, roots, RPC health, contracts, and watch status.\nUse this when a relay or token transfer is stuck.\nExample: cast-interop debug tx --chain era 0xTX"
)]
pub struct DebugCommand {
    #[command(subcommand)]
    pub command: DebugSubcommand,
}

/// Debug subcommands.
#[derive(Subcommand, Debug)]
pub enum DebugSubcommand {
    #[command(
        about = "Decode interop events from a transaction.",
        long_about = "Fetch the transaction receipt and decode interop-related events.\nUse this to confirm bundle hashes and event data.\nExample: cast-interop debug tx --chain era 0xTX_HASH"
    )]
    Tx(TxShowArgs),
    #[command(
        about = "Fetch a log proof for an interop transaction.",
        long_about = "Wait for finalization and fetch the L2→L1 log proof (getLogProof).\nUse this when you need a proof for bundle verify/execute.\nExample: cast-interop debug proof --chain era --tx 0xTX_HASH"
    )]
    Proof(ProofArgs),
    #[command(
        about = "Wait for an interop root on the destination chain.",
        long_about = "Poll interopRoots(chainId, batchNumber) until the expected root appears.\nUse this after the source log proof is available.\nExample: cast-interop debug root --chain test --source-chain 324 --batch 123 --expected-root 0xROOT"
    )]
    Root(RootWaitArgs),
    #[command(
        about = "Check RPC feature support.",
        long_about = "Ping the RPC to detect zkSync-specific methods and finalized blocks.\nUse this to validate an RPC before running other commands.\nExample: cast-interop debug rpc --chain era"
    )]
    Rpc(RpcPingArgs),
    #[command(
        about = "Inspect configured interop contract addresses.",
        long_about = "Print the interop contracts resolved from config and flags.\nUse this to confirm the right addresses are being used.\nExample: cast-interop debug contracts --chain era"
    )]
    Contracts(ContractsArgs),
    #[command(
        about = "Run a diagnostics checklist.",
        long_about = "Validate RPC connectivity, ABI availability, and address configuration.\nUse this before debugging a failing relay.\nExample: cast-interop debug doctor --chain era"
    )]
    Doctor(DoctorArgs),
    #[command(
        about = "Watch a transaction until a target state.",
        long_about = "Poll for finalization, log proof availability, root propagation, and bundle status.\nUse this to monitor relay progress over time.\nExample: cast-interop debug watch --chain-src era --chain-dest test --tx 0xTX_HASH --until executed"
    )]
    Watch(WatchArgs),
}

impl DebugCommand {
    /// Run the selected debug workflow.
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            DebugSubcommand::Tx(args) => commands::tx_show::run(args, config, addresses).await,
            DebugSubcommand::Proof(args) => commands::proof::run(args, config, addresses).await,
            DebugSubcommand::Root(args) => commands::root_wait::run(args, config, addresses).await,
            DebugSubcommand::Rpc(args) => commands::rpc_ping::run(args, config, addresses).await,
            DebugSubcommand::Contracts(args) => {
                commands::contracts::run(args, config, addresses).await
            }
            DebugSubcommand::Doctor(args) => commands::doctor::run(args, config, addresses).await,
            DebugSubcommand::Watch(args) => commands::watch::run(args, config, addresses).await,
        }
    }
}

/// Bundle lifecycle commands.
#[derive(Parser, Debug)]
#[command(
    about = "Bundle lifecycle workflows.",
    long_about = "Extract, verify, execute, relay, and explain interop bundles.\nUse this to move bundles across chains or inspect their status.\nExample: cast-interop bundle extract --chain era --tx 0xTX_HASH"
)]
pub struct BundleCommand {
    #[command(subcommand)]
    pub command: BundleSubcommand,
}

/// Bundle subcommands.
#[derive(Subcommand, Debug)]
pub enum BundleSubcommand {
    #[command(
        about = "Extract an interop bundle from a transaction.",
        long_about = "Locate the InteropBundleSent event and write the encoded bundle.\nUse this to prepare manual verify/execute steps.\nExample: cast-interop bundle extract --chain era --tx 0xTX_HASH --out bundle.hex"
    )]
    Extract(BundleExtractArgs),
    #[command(
        about = "Verify a bundle on the destination chain.",
        long_about = "Submit a bundle proof to mark it verified on the handler contract.\nUse this before executing a bundle.\nExample: cast-interop bundle verify --chain test --bundle bundle.hex --proof proof.json --private-key $PRIVATE_KEY"
    )]
    Verify(BundleActionArgs),
    #[command(
        about = "Execute a bundle on the destination chain.",
        long_about = "Submit a bundle proof and execute calls on the handler contract.\nUse this to finalize bundle delivery.\nExample: cast-interop bundle execute --chain test --bundle bundle.hex --proof proof.json --private-key $PRIVATE_KEY"
    )]
    Execute(BundleActionArgs),
    #[command(
        about = "Check bundle status on the destination chain.",
        long_about = "Fetch the bundle status and optional call status from the handler.\nUse this after verify/execute to confirm progress.\nExample: cast-interop bundle status --chain test --bundle-hash 0xBUNDLE_HASH"
    )]
    Status(StatusArgs),
    #[command(
        about = "Explain a bundle proof execution.",
        long_about = "Simulate bundle verification or execution and decode any revert reason.\nUse this to debug failed handler transactions.\nExample: cast-interop bundle explain --chain test --bundle bundle.hex --proof proof.json --private-key $PRIVATE_KEY"
    )]
    Explain(ExplainArgs),
    #[command(
        about = "Relay a bundle end-to-end.",
        long_about = "Fetch proof from source, wait for root, and verify/execute on destination.\nUse this to automate the full relay flow.\nExample: cast-interop bundle relay --chain-src era --chain-dest test --tx 0xTX_HASH --mode execute --private-key $PRIVATE_KEY"
    )]
    Relay(RelayArgs),
}

impl BundleCommand {
    /// Run the selected bundle workflow.
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
            BundleSubcommand::Status(args) => commands::status::run(args, config, addresses).await,
            BundleSubcommand::Explain(args) => {
                commands::explain::run(args, config, addresses).await
            }
            BundleSubcommand::Relay(args) => commands::relay::run(args, config, addresses).await,
        }
    }
}

/// Send interop messages or bundles.
#[derive(Parser, Debug)]
#[command(
    about = "Send interop messages or bundles.",
    long_about = "Create and send interop messages or bundles from a source chain.\nUse this when you need to author a new interop payload.\nExample: cast-interop send message --chain era --to-chain test --to 0xTARGET --payload 0xdeadbeef --private-key $PRIVATE_KEY"
)]
pub struct SendCommand {
    #[command(subcommand)]
    pub command: SendSubcommand,
}

/// Send subcommands.
#[derive(Subcommand, Debug)]
pub enum SendSubcommand {
    #[command(
        about = "Send a single interop message.",
        long_about = "Build and send a single interop message to a destination chain.\nUse this for one-off calls or pings.\nExample: cast-interop send message --chain era --to-chain test --to 0xTARGET --payload 0xdeadbeef --private-key $PRIVATE_KEY"
    )]
    Message(SendMessageArgs),
    #[command(
        about = "Send a bundle of interop calls.",
        long_about = "Send a bundle with multiple calls described in a JSON file.\nUse this when you need batching on the destination chain.\nExample: cast-interop send bundle --chain era --to-chain test --calls calls.json --private-key $PRIVATE_KEY"
    )]
    Bundle(SendBundleArgs),
}

impl SendCommand {
    /// Run the selected send workflow.
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

/// Token bridging workflows.
#[derive(Parser, Debug)]
#[command(
    about = "Token bridging workflows.",
    long_about = "Inspect wrapped tokens, check balances, or send ERC20s across chains.\nUse this when bridging assets through interop.\nExample: cast-interop token send --chain-src era --chain-dest test --token 0xTOKEN --amount 1 --to 0xRECIPIENT --private-key $PRIVATE_KEY"
)]
pub struct TokenCommand {
    #[command(subcommand)]
    pub command: TokenSubcommand,
}

/// Token subcommands.
#[derive(Subcommand, Debug)]
pub enum TokenSubcommand {
    #[command(
        about = "Get wrapped token info.",
        long_about = "Resolve the wrapped token address and metadata on the destination chain.\nUse this before sending to confirm the wrapped token exists.\nExample: cast-interop token info --chain-src era --chain-dest test --token 0xTOKEN"
    )]
    Info(TokenInfoArgs),
    #[command(
        about = "Check a destination balance.",
        long_about = "Look up the wrapped token balance for a recipient on the destination chain.\nUse this to verify token delivery.\nExample: cast-interop token balance --chain-src era --chain-dest test --token 0xTOKEN --to 0xRECIPIENT"
    )]
    Balance(TokenBalanceArgs),
    #[command(
        about = "Send a token across chains.",
        long_about = "Send an ERC20 across chains via interop (Type B flow).\nUse this for cross-chain token transfers, with optional watch mode.\nExample: cast-interop token send --chain-src era --chain-dest test --token 0xTOKEN --amount 1 --to 0xRECIPIENT --private-key $PRIVATE_KEY"
    )]
    Send(TokenSendArgs),
}

impl TokenCommand {
    /// Run the selected token workflow.
    pub async fn run(self, config: Config, addresses: AddressBook) -> Result<()> {
        match self.command {
            TokenSubcommand::Info(args) => commands::token::run_info(args, config, addresses).await,
            TokenSubcommand::Balance(args) => {
                commands::token::run_balance(args, config, addresses).await
            }
            TokenSubcommand::Send(args) => commands::token::run_send(args, config, addresses).await,
        }
    }
}

/// Encoding utilities.
#[derive(Parser, Debug)]
#[command(
    about = "Encoding utilities.",
    long_about = "Encode ERC-7930 addresses, call attributes, or asset IDs.\nUse this for low-level data preparation and inspection.\nExample: cast-interop encode 7930 --chain-id 324 --address 0xADDRESS"
)]
pub struct EncodeCommand {
    #[command(subcommand)]
    pub command: EncodeSubcommand,
}

/// Encode subcommands.
#[derive(Subcommand, Debug)]
pub enum EncodeSubcommand {
    #[command(
        name = "7930",
        about = "Encode ERC-7930 chain/address bytes.",
        long_about = "Encode chain/address references into ERC-7930 bytes.\nUse this when building interop call attributes.\nExample: cast-interop encode 7930 --chain-id 324 --address 0xADDRESS"
    )]
    Erc7930(Encode7930Args),
    #[command(
        about = "Encode interop call attributes.",
        long_about = "Build the attribute list for interop calls (interop value, indirect value, etc.).\nUse this to precompute bundle attributes.\nExample: cast-interop encode attrs --interop-value 0 --execution-address permissionless"
    )]
    Attrs(EncodeAttrsArgs),
    #[command(
        name = "asset-id",
        about = "Encode an interop asset ID.",
        long_about = "Compute the assetId hash from chain ID, token, and vault.\nUse this for token bridging diagnostics.\nExample: cast-interop encode asset-id --chain-id 324 --token 0xTOKEN"
    )]
    AssetId(EncodeAssetIdArgs),
}

impl EncodeCommand {
    /// Run the selected encoding utility.
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

/// Manage configured chain aliases.
#[derive(Parser, Debug)]
#[command(
    about = "Manage configured chains.",
    long_about = "Add, list, or remove chain aliases in the config file.\nUse this to avoid repeating RPC URLs.\nExample: cast-interop chains add era --rpc https://mainnet.era.zksync.io"
)]
pub struct ChainsCommand {
    #[command(subcommand)]
    pub command: ChainsSubcommand,
}

/// Chain configuration subcommands.
#[derive(Subcommand, Debug)]
pub enum ChainsSubcommand {
    #[command(
        about = "List configured chains.",
        long_about = "Print the configured chains with chain IDs and RPC URLs.\nUse this to confirm aliases in your config.\nExample: cast-interop chains list"
    )]
    List(ChainsListArgs),
    #[command(
        about = "Add a chain alias.",
        long_about = "Store a chain alias with its RPC URL and chain ID.\nUse this to simplify future CLI commands.\nExample: cast-interop chains add era --rpc https://mainnet.era.zksync.io"
    )]
    Add(ChainsAddArgs),
    #[command(
        about = "Remove a chain alias.",
        long_about = "Delete a chain alias from the config file.\nUse this to clean up outdated entries.\nExample: cast-interop chains rm era"
    )]
    Rm(ChainsRemoveArgs),
}

impl ChainsCommand {
    /// Run the selected chain configuration command.
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

/// Shared RPC selection flags.
#[derive(Args, Debug, Clone)]
pub struct RpcSelectionArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "RPC URL to use. Use instead of --chain. Default: uses the configured default chain if set."
    )]
    pub rpc: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Configured chain alias. Use instead of --rpc. Default: uses the configured default chain if set."
    )]
    pub chain: Option<String>,
}

/// Signer selection flags for sending transactions.
#[derive(Args, Debug, Clone)]
pub struct SignerArgs {
    #[arg(
        long,
        value_name = "HEX",
        help = "Private key hex string. Use instead of --private-key-env. Default: unset."
    )]
    pub private_key: Option<String>,

    #[arg(
        long,
        value_name = "ENV",
        help = "Environment variable holding the private key. Default: PRIVATE_KEY or config signer.private_key_env."
    )]
    pub private_key_env: Option<String>,
}

/// Decode interop events from a transaction receipt.
#[derive(Args, Debug)]
pub struct TxShowArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(value_name = "TX_HASH", help = "Transaction hash to decode.")]
    pub tx_hash: String,

    #[arg(long, help = "Only show interop-specific events. Default: false.")]
    pub interop_only: bool,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Extract a bundle from an interop transaction.
#[derive(Args, Debug)]
pub struct BundleExtractArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "TX_HASH",
        help = "Transaction hash to extract from."
    )]
    pub tx: String,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the encoded bundle hex to a file. Default: unset."
    )]
    pub out: Option<PathBuf>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the JSON bundle view to a file. Default: unset."
    )]
    pub json_out: Option<PathBuf>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Fetch a log proof for an interop transaction.
#[derive(Args, Debug)]
pub struct ProofArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long, value_name = "TX_HASH", help = "Transaction hash to prove.")]
    pub tx: String,

    #[arg(
        long,
        value_name = "INDEX",
        default_value_t = 0,
        help = "Message index within the transaction. Default: 0."
    )]
    pub msg_index: u32,

    #[arg(
        long,
        value_name = "PATH",
        help = "Write the proof JSON to a file. Default: unset."
    )]
    pub out: Option<PathBuf>,

    #[arg(
        long,
        help = "Do not wait for finalization before fetching proof. Default: false."
    )]
    pub no_wait: bool,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Timeout while waiting for proof availability. Default: 300000."
    )]
    pub timeout_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Polling interval for proof availability. Default: 1000."
    )]
    pub poll_ms: Option<u64>,
}

/// Wait for an interop root on the destination chain.
#[derive(Args, Debug)]
pub struct RootWaitArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "CHAIN_ID",
        help = "Source chain ID (not alias) used in interopRoots."
    )]
    pub source_chain: String,

    #[arg(long, value_name = "BATCH", help = "L1 batch number to check.")]
    pub batch: u64,

    #[arg(
        long,
        value_name = "ROOT",
        help = "Expected interop root value (0x...)."
    )]
    pub expected_root: String,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Timeout while waiting for root availability. Default: 300000."
    )]
    pub timeout_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Polling interval while waiting for root. Default: 1000."
    )]
    pub poll_ms: Option<u64>,
}

/// Verify or execute a bundle on the destination chain.
#[derive(Args, Debug)]
pub struct BundleActionArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "HEX_OR_PATH",
        help = "Encoded bundle hex string or path to a bundle file."
    )]
    pub bundle: String,

    #[arg(
        long,
        value_name = "JSON_OR_PATH",
        help = "Bundle proof JSON string or path to proof file."
    )]
    pub proof: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop handler address. Default: config addresses.interop_handler."
    )]
    pub handler: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop center address. Default: config addresses.interop_center."
    )]
    pub center: Option<String>,

    #[arg(
        long,
        help = "Simulate the call without sending a transaction. Default: false."
    )]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,
}

/// Check bundle status on the destination chain.
#[derive(Args, Debug)]
pub struct StatusArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long, value_name = "BUNDLE_HASH", help = "Bundle hash to query.")]
    pub bundle_hash: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop handler address. Default: config addresses.interop_handler."
    )]
    pub handler: Option<String>,

    #[arg(
        long,
        value_name = "HEX_OR_PATH",
        help = "Optional bundle hex for per-call status lookup. Default: unset."
    )]
    pub bundle: Option<String>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Relay a bundle end-to-end across chains.
#[derive(Args, Debug)]
pub struct RelayArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Source chain RPC URL. Use instead of --chain-src. Default: uses configured default chain if set."
    )]
    pub rpc_src: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Source chain alias. Use instead of --rpc-src. Default: uses configured default chain if set."
    )]
    pub chain_src: Option<String>,

    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Destination chain RPC URL. Use instead of --chain-dest. Default: uses configured default chain if set."
    )]
    pub rpc_dest: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Destination chain alias. Use instead of --rpc-dest. Default: uses configured default chain if set."
    )]
    pub chain_dest: Option<String>,

    #[arg(long, value_name = "TX_HASH", help = "Source transaction hash.")]
    pub tx: String,

    #[arg(
        long,
        value_name = "INDEX",
        default_value_t = 0,
        help = "Message index within the transaction. Default: 0."
    )]
    pub msg_index: u32,

    #[arg(
        long,
        value_name = "MODE",
        default_value = "execute",
        help = "Relay mode (execute or verify). Default: execute."
    )]
    pub mode: String,

    #[arg(
        long,
        value_name = "DIR",
        help = "Directory to write relay artifacts. Default: unset."
    )]
    pub out_dir: Option<PathBuf>,

    #[arg(
        long,
        help = "Simulate the relay without sending transactions. Default: false."
    )]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop handler address. Default: config addresses.interop_handler."
    )]
    pub handler: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop center address. Default: config addresses.interop_center."
    )]
    pub center: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop root storage address. Default: config addresses.interop_root_storage."
    )]
    pub root_storage: Option<String>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Timeout while waiting for proof/root. Default: 300000."
    )]
    pub timeout_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Polling interval for proof/root. Default: 1000."
    )]
    pub poll_ms: Option<u64>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// List configured chains.
#[derive(Args, Debug)]
pub struct ChainsListArgs {
    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Add a chain alias.
#[derive(Args, Debug)]
pub struct ChainsAddArgs {
    #[arg(value_name = "ALIAS", help = "Alias name to store.")]
    pub alias: String,

    #[arg(long, value_name = "RPC_URL", help = "RPC URL for the chain.")]
    pub rpc: String,
}

/// Remove a chain alias.
#[derive(Args, Debug)]
pub struct ChainsRemoveArgs {
    #[arg(value_name = "ALIAS", help = "Alias name to remove.")]
    pub alias: String,
}

/// Check RPC capabilities and status.
#[derive(Args, Debug)]
pub struct RpcPingArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Print interop contract addresses.
#[derive(Args, Debug)]
pub struct ContractsArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Send a single interop message.
#[derive(Args, Debug)]
pub struct SendMessageArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "CHAIN_ID",
        help = "Destination chain ID (not alias)."
    )]
    pub to_chain: String,

    #[arg(long, value_name = "ADDRESS", help = "Target contract address.")]
    pub to: String,

    #[arg(
        long,
        value_name = "HEX",
        help = "Hex payload data. Use instead of --payload-file. Required unless --payload-file is set."
    )]
    pub payload: Option<String>,

    #[arg(
        long,
        value_name = "PATH",
        help = "Path to a hex payload file. Use instead of --payload. Required unless --payload is set."
    )]
    pub payload_file: Option<PathBuf>,

    #[arg(
        long,
        value_name = "WEI",
        help = "Interop call value in wei. Default: 0."
    )]
    pub interop_value: Option<String>,

    #[arg(
        long,
        value_name = "WEI",
        help = "Indirect message value in wei. Default: 0."
    )]
    pub indirect: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Execution address or 'permissionless'. Default: unset (permissionless)."
    )]
    pub execution_address: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Unbundler address for the call. Default: unset."
    )]
    pub unbundler: Option<String>,

    #[arg(
        long,
        help = "Simulate the message without sending a transaction. Default: false."
    )]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Send a bundle of interop calls.
#[derive(Args, Debug)]
pub struct SendBundleArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "CHAIN_ID",
        help = "Destination chain ID (not alias)."
    )]
    pub to_chain: String,

    #[arg(long, value_name = "PATH", help = "Path to bundle calls JSON.")]
    pub calls: PathBuf,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Bundle execution address or 'permissionless'. Default: unset."
    )]
    pub bundle_execution_address: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Bundle unbundler address. Default: unset."
    )]
    pub bundle_unbundler: Option<String>,

    #[arg(
        long,
        help = "Simulate the bundle without sending a transaction. Default: false."
    )]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Resolve wrapped token metadata.
#[derive(Args, Debug)]
pub struct TokenInfoArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Source chain RPC URL. Use instead of --chain-src. Default: uses configured default chain if set."
    )]
    pub rpc_src: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Source chain alias. Use instead of --rpc-src. Default: uses configured default chain if set."
    )]
    pub chain_src: Option<String>,

    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Destination chain RPC URL. Use instead of --chain-dest. Default: uses configured default chain if set."
    )]
    pub rpc_dest: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Destination chain alias. Use instead of --rpc-dest. Default: uses configured default chain if set."
    )]
    pub chain_dest: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Token address on the source chain."
    )]
    pub token: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Native token vault address. Default: 0x0000000000000000000000000000000000010004."
    )]
    pub native_token_vault: Option<String>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Check wrapped token balances.
#[derive(Args, Debug)]
pub struct TokenBalanceArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Source chain RPC URL. Use instead of --chain-src. Default: uses configured default chain if set."
    )]
    pub rpc_src: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Source chain alias. Use instead of --rpc-src. Default: uses configured default chain if set."
    )]
    pub chain_src: Option<String>,

    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Destination chain RPC URL. Use instead of --chain-dest. Default: uses configured default chain if set."
    )]
    pub rpc_dest: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Destination chain alias. Use instead of --rpc-dest. Default: uses configured default chain if set."
    )]
    pub chain_dest: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Token address on the source chain."
    )]
    pub token: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Recipient address on the destination chain."
    )]
    pub to: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Native token vault address. Default: 0x0000000000000000000000000000000000010004."
    )]
    pub native_token_vault: Option<String>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Send a token across chains.
#[derive(Args, Debug)]
pub struct TokenSendArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Source chain RPC URL. Use instead of --chain-src. Default: uses configured default chain if set."
    )]
    pub rpc_src: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Source chain alias. Use instead of --rpc-src. Default: uses configured default chain if set."
    )]
    pub chain_src: Option<String>,

    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Destination chain RPC URL. Use instead of --chain-dest. Default: uses configured default chain if set."
    )]
    pub rpc_dest: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Destination chain alias. Use instead of --rpc-dest. Default: uses configured default chain if set."
    )]
    pub chain_dest: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Token address on the source chain."
    )]
    pub token: String,

    #[arg(
        long,
        value_name = "AMOUNT",
        help = "Token amount in human units (uses decimals). Use instead of --amount-wei. Requires --decimals if token decimals are unavailable."
    )]
    pub amount: Option<String>,

    #[arg(
        long,
        value_name = "WEI",
        help = "Token amount in wei (raw). Use instead of --amount. Default: unset."
    )]
    pub amount_wei: Option<String>,

    #[arg(
        long,
        value_name = "DECIMALS",
        help = "Token decimals used with --amount. Default: fetched from token; required if decimals are unavailable."
    )]
    pub decimals: Option<u32>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Recipient address on destination."
    )]
    pub to: String,

    #[arg(
        long,
        value_name = "WEI",
        default_value = "0",
        help = "Indirect message value in wei. Default: 0."
    )]
    pub indirect_msg_value: String,

    #[arg(
        long,
        value_name = "WEI",
        help = "Interop call value in wei. Default: none."
    )]
    pub interop_value: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Unbundler address on destination. Default: recipient."
    )]
    pub unbundler: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Asset router address. Default: 0x0000000000000000000000000000000000010003."
    )]
    pub asset_router: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Native token vault address. Default: 0x0000000000000000000000000000000000010004."
    )]
    pub native_token_vault: Option<String>,

    #[arg(long, help = "Skip token registration step. Default: false.")]
    pub skip_register: bool,

    #[arg(long, help = "Skip token approve step. Default: false.")]
    pub skip_approve: bool,

    #[arg(
        long,
        value_name = "AMOUNT",
        help = "Approve amount (wei or 'infinite'). Default: equals send amount."
    )]
    pub approve_amount: Option<String>,

    #[arg(
        long,
        value_name = "MODE",
        default_value = "execute",
        help = "Handler action (execute or verify). Default: execute."
    )]
    pub mode: String,

    #[arg(long, help = "Watch the relay flow until completion. Default: false.")]
    pub watch: bool,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Timeout while waiting for proof/root. Default: 300000."
    )]
    pub timeout_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Polling interval for proof/root. Default: 1000."
    )]
    pub poll_ms: Option<u64>,

    #[arg(
        long,
        help = "Simulate the token transfer without sending transactions. Default: false."
    )]
    pub dry_run: bool,

    #[command(flatten)]
    pub signer: SignerArgs,
}

/// Encode ERC-7930 bytes.
#[derive(Args, Debug)]
pub struct Encode7930Args {
    #[arg(
        long,
        value_name = "CHAIN_ID",
        help = "Chain ID to encode. Required unless --address-only is set."
    )]
    pub chain_id: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Address to encode with the chain ID. Default: none."
    )]
    pub address: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Encode only the address without chain ID. Use instead of --chain-id/--address."
    )]
    pub address_only: Option<String>,
}

/// Encode interop attributes.
#[derive(Args, Debug)]
pub struct EncodeAttrsArgs {
    #[arg(
        long,
        value_name = "WEI",
        help = "Interop call value in wei. Default: none."
    )]
    pub interop_value: Option<String>,

    #[arg(
        long,
        value_name = "WEI",
        help = "Indirect message value in wei. Default: none."
    )]
    pub indirect: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Execution address or 'permissionless'. Default: unset (permissionless)."
    )]
    pub execution_address: Option<String>,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Unbundler address. Default: none."
    )]
    pub unbundler: Option<String>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Encode a token asset ID.
#[derive(Args, Debug)]
pub struct EncodeAssetIdArgs {
    #[arg(long, value_name = "CHAIN_ID", help = "Source chain ID.")]
    pub chain_id: String,

    #[arg(long, value_name = "ADDRESS", help = "Token address on source chain.")]
    pub token: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Native token vault address. Default: 0x0000000000000000000000000000000000010004."
    )]
    pub native_token_vault: Option<String>,
}

/// Watch interop progress.
#[derive(Args, Debug)]
pub struct WatchArgs {
    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Source chain RPC URL. Use instead of --chain-src. Default: uses configured default chain if set."
    )]
    pub rpc_src: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Source chain alias. Use instead of --rpc-src. Default: uses configured default chain if set."
    )]
    pub chain_src: Option<String>,

    #[arg(
        long,
        value_name = "RPC_URL",
        help = "Destination chain RPC URL. Use instead of --chain-dest. Default: uses configured default chain if set."
    )]
    pub rpc_dest: Option<String>,

    #[arg(
        long,
        value_name = "CHAIN",
        help = "Destination chain alias. Use instead of --rpc-dest. Default: uses configured default chain if set."
    )]
    pub chain_dest: Option<String>,

    #[arg(
        long,
        value_name = "TX_HASH",
        help = "Source transaction hash to watch."
    )]
    pub tx: String,

    #[arg(
        long,
        value_name = "INDEX",
        default_value_t = 0,
        help = "Message index within the transaction. Default: 0."
    )]
    pub msg_index: u32,

    #[arg(
        long,
        value_name = "STATE",
        help = "Stop when the bundle is verified or executed. Values: verified|executed. Default: unset."
    )]
    pub until: Option<String>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Polling interval while watching. Default: 1000."
    )]
    pub poll_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        help = "Timeout while watching. Default: 300000."
    )]
    pub timeout_ms: Option<u64>,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Run diagnostic checks.
#[derive(Args, Debug)]
pub struct DoctorArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}

/// Explain a bundle proof execution.
#[derive(Args, Debug)]
pub struct ExplainArgs {
    #[command(flatten)]
    pub rpc: RpcSelectionArgs,

    #[arg(
        long,
        value_name = "HEX_OR_PATH",
        help = "Encoded bundle hex string or path to a bundle file."
    )]
    pub bundle: String,

    #[arg(
        long,
        value_name = "JSON_OR_PATH",
        help = "Bundle proof JSON string or path to proof file."
    )]
    pub proof: String,

    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Override the interop handler address. Default: config addresses.interop_handler."
    )]
    pub handler: Option<String>,

    #[command(flatten)]
    pub signer: SignerArgs,

    #[arg(long, help = "Emit JSON output. Default: false.")]
    pub json: bool,
}
