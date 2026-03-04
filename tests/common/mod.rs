use alloy_primitives::{Address, TxKind};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{TransactionInput, TransactionRequest};
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::SolValue;
use anyhow::{Context, Result};
use serde_json::Value;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_PRIVATE_KEY: &str =
    "0x7726827caac94a7f9e1b160f7ea819f172f7b6f9d2a97f992c38edeab82d4110";
const MESSAGE_SELECTOR: [u8; 4] = [0xe2, 0x1f, 0x37, 0xce];

#[derive(Debug, Clone)]
pub struct TestEnv {
    pub l1_rpc: String,
    pub rpc_a: String,
    pub rpc_b: String,
    pub chain_a_id: u64,
    pub chain_b_id: u64,
    pub private_key: String,
}

#[derive(Debug)]
pub struct CommandResult {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    pub fn success_json(&self) -> Result<Value> {
        if self.status_code != Some(0) {
            anyhow::bail!(
                "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
                self.status_code,
                self.stdout,
                self.stderr
            );
        }
        let trimmed = self.stdout.trim();
        if let Ok(value) = serde_json::from_str(trimmed) {
            return Ok(value);
        }
        if let Some(start) = trimmed.rfind("\n{") {
            let json = &trimmed[(start + 1)..];
            if let Ok(value) = serde_json::from_str(json) {
                return Ok(value);
            }
        }
        serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse command JSON output\nstdout:\n{}\nstderr:\n{}",
                self.stdout, self.stderr
            )
        })
    }

    pub fn combined_output(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

impl TestEnv {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            l1_rpc: required_env("CAST_INTEROP_L1_RPC")?,
            rpc_a: required_env("CAST_INTEROP_RPC_A")?,
            rpc_b: required_env("CAST_INTEROP_RPC_B")?,
            chain_a_id: required_env("CAST_INTEROP_CHAIN_A_ID")?
                .parse()
                .context("CAST_INTEROP_CHAIN_A_ID must be an integer")?,
            chain_b_id: required_env("CAST_INTEROP_CHAIN_B_ID")?
                .parse()
                .context("CAST_INTEROP_CHAIN_B_ID must be an integer")?,
            private_key: env::var("CAST_INTEROP_E2E_PRIVATE_KEY")
                .unwrap_or_else(|_| DEFAULT_PRIVATE_KEY.to_string()),
        })
    }
}

pub fn bin_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("CAST_INTEROP_BIN") {
        return Ok(PathBuf::from(path));
    }
    env::var("CARGO_BIN_EXE_cast-interop")
        .map(PathBuf::from)
        .context("CAST_INTEROP_BIN or CARGO_BIN_EXE_cast-interop must be set")
}

pub fn run_cli(args: &[&str]) -> Result<CommandResult> {
    let output = Command::new(bin_path()?)
        .args(args)
        .output()
        .with_context(|| format!("failed to run cast-interop with args {args:?}"))?;

    Ok(CommandResult {
        status_code: output.status.code(),
        stdout: String::from_utf8(output.stdout).context("stdout was not valid UTF-8")?,
        stderr: String::from_utf8(output.stderr).context("stderr was not valid UTF-8")?,
    })
}

pub fn temp_test_dir(test_name: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH")?
        .as_millis();
    let path = env::temp_dir().join(format!(
        "cast-interop-{test_name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path)
        .with_context(|| format!("failed to create temp dir {}", path.display()))?;
    Ok(path)
}

pub async fn wait_for_rpc_chain_id(url: &str, expected: u64, timeout: Duration) -> Result<()> {
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(chain_id) = fetch_chain_id(&client, url).await {
            if chain_id == expected {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    anyhow::bail!("rpc {url} did not report chain id {expected} in time")
}

pub async fn fetch_chain_id(client: &reqwest::Client, url: &str) -> Result<u64> {
    let response = json_rpc(client, url, "eth_chainId", serde_json::json!([])).await?;
    parse_hex_u64(response)
}

pub fn assert_file_exists(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    anyhow::bail!("expected file {} to exist", path.display())
}

pub async fn deploy_greeting_contract(rpc_url: &str, private_key: &str) -> Result<Address> {
    let signer: PrivateKeySigner = private_key
        .parse()
        .context("failed to parse test private key for contract deployment")?;
    let chain_id = fetch_chain_id(&reqwest::Client::new(), rpc_url).await?;
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .with_chain_id(chain_id)
        .connect(rpc_url)
        .await
        .with_context(|| format!("failed to connect signer to {rpc_url}"))?;
    let max_fee_per_gas = provider
        .get_gas_price()
        .await
        .with_context(|| format!("failed to get gas price from {rpc_url}"))?;
    let max_fee_per_gas = max_fee_per_gas + max_fee_per_gas;
    let request = TransactionRequest {
        to: Some(TxKind::Create),
        input: TransactionInput::new(greeting_deployment_data()?.into()),
        max_fee_per_gas: Some(max_fee_per_gas),
        max_priority_fee_per_gas: Some(0),
        ..Default::default()
    };
    let pending = provider
        .send_transaction(request)
        .await
        .with_context(|| format!("failed to submit Greeting deployment to {rpc_url}"))?;
    let receipt = pending
        .get_receipt()
        .await
        .with_context(|| format!("failed to fetch Greeting deployment receipt from {rpc_url}"))?;
    let Some(address) = receipt.contract_address else {
        anyhow::bail!("Greeting deployment on {rpc_url} did not return a contract address");
    };
    Ok(address)
}

pub async fn read_message(rpc_url: &str, contract: Address) -> Result<String> {
    let request = serde_json::json!([{
        "to": format!("{contract:#x}"),
        "data": format!("0x{}", hex::encode(MESSAGE_SELECTOR)),
    }, "latest"]);
    let result = json_rpc(&reqwest::Client::new(), rpc_url, "eth_call", request).await?;
    let response = result
        .as_str()
        .context("expected hex string JSON-RPC result for message()")?
        .to_string();
    let bytes = decode_hex_string(&response)?;
    let value = String::abi_decode(bytes.as_ref())
        .with_context(|| format!("failed to decode message() response {response}"))?;
    Ok(value)
}

pub fn encode_message_payload(next_message: &str) -> String {
    format!(
        "0x{}",
        hex::encode((next_message.to_string(),).abi_encode())
    )
}

pub async fn fetch_receipt_status(rpc_url: &str, tx_hash: &str) -> Result<Option<bool>> {
    let receipt = json_rpc(
        &reqwest::Client::new(),
        rpc_url,
        "eth_getTransactionReceipt",
        serde_json::json!([tx_hash]),
    )
    .await?;
    if receipt.is_null() {
        return Ok(None);
    }
    let status = receipt
        .get("status")
        .and_then(Value::as_str)
        .context("transaction receipt missing status")?;
    let trimmed = status.trim_start_matches("0x");
    let value = u64::from_str_radix(trimmed, 16)
        .with_context(|| format!("failed to parse receipt status from {status}"))?;
    Ok(Some(value == 1))
}

async fn json_rpc(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let response = client
        .post(url)
        .timeout(Duration::from_secs(5))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .with_context(|| format!("rpc request to {url} failed"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .context("rpc response was not valid JSON")?;
    if !status.is_success() {
        anyhow::bail!("rpc request to {url} returned HTTP {status}: {body}");
    }
    if let Some(error) = body.get("error") {
        anyhow::bail!("rpc request to {url} returned error: {error}");
    }
    body.get("result")
        .cloned()
        .context("rpc response missing result")
}

fn parse_hex_u64(value: Value) -> Result<u64> {
    let text = value
        .as_str()
        .context("expected hex string JSON-RPC result")?;
    let trimmed = text.trim_start_matches("0x");
    u64::from_str_radix(trimmed, 16)
        .with_context(|| format!("failed to parse hex integer from {text}"))
}

fn greeting_deployment_data() -> Result<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("deps")
        .join("Greeting.json");
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let artifact: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let bytecode = artifact
        .get("bytecode")
        .and_then(|value| value.get("object"))
        .and_then(Value::as_str)
        .context("deps/Greeting.json missing bytecode.object")?;
    decode_hex_string(bytecode)
}

fn decode_hex_string(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim_start_matches("0x");
    hex::decode(trimmed).with_context(|| format!("failed to decode hex string {value}"))
}

fn required_env(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} is required for the Docker E2E harness"))
}
