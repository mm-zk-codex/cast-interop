use crate::abi::{encode_execute_bundle_call, encode_verify_bundle_call, error_selector_map};
use crate::cli::BundleActionArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{
    require_signer_or_dry_run, AddressBook, MessageInclusionProof, BUNDLE_IDENTIFIER,
};
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::transport::TransportResult;
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionInput;
use alloy_sol_types::SolValue;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::str::FromStr;

/// Verify a bundle proof on the destination chain.
///
/// This submits a verify call and reports the transaction hash or dry-run result.
pub async fn run_verify(
    args: BundleActionArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    run_bundle_action("bundle verify", args, config, addresses, true).await
}

/// Execute a bundle proof on the destination chain.
///
/// This submits an execute call and reports the transaction hash or dry-run result.
pub async fn run_execute(
    args: BundleActionArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    run_bundle_action("bundle execute", args, config, addresses, false).await
}

/// Shared implementation for bundle verify/execute flows.
///
/// Handles proof loading, sender normalization, and dry-run behavior.
async fn run_bundle_action(
    cmd: &str,
    args: BundleActionArgs,
    config: Config,
    addresses: AddressBook,
    is_verify: bool,
) -> Result<()> {
    let handler = args
        .handler
        .as_deref()
        .map(|value| Address::from_str(value))
        .transpose()
        .context("invalid handler address")?
        .unwrap_or(addresses.interop_handler);
    let center = args
        .center
        .as_deref()
        .map(|value| Address::from_str(value))
        .transpose()
        .context("invalid center address")?
        .unwrap_or(addresses.interop_center);

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;

    require_signer_or_dry_run(wallet.is_some(), args.dry_run, cmd)?;

    let encoded_bundle = load_hex_or_path(&args.bundle)?;
    let mut proof = load_proof(&args.proof)?;

    let expected_sender = format!("{center:#x}");
    if proof.message.sender.to_lowercase() != expected_sender.to_lowercase() {
        eprintln!(
            "warning: overriding proof sender {} -> {}",
            proof.message.sender, expected_sender
        );
    }
    proof.message.sender = expected_sender;
    proof.message.data = format!(
        "0x{}{}",
        hex::encode([BUNDLE_IDENTIFIER]),
        hex::encode(&encoded_bundle)
    );

    let calldata = if is_verify {
        encode_verify_bundle_call(Bytes::from(encoded_bundle.clone()), proof.clone())?
    } else {
        encode_execute_bundle_call(Bytes::from(encoded_bundle.clone()), proof.clone())?
    };

    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    if args.dry_run {
        match eth_call(&client, handler, calldata.clone()).await {
            Ok(_) => {
                println!("dry-run success");
            }
            Err(err) => {
                if let Some(reason) = decode_revert_reason(err.to_string()) {
                    println!("dry-run revert: {reason}");
                } else {
                    println!("dry-run failed: {err}");
                }
            }
        }
        return Ok(());
    }

    let wallet = wallet.expect("wallet required");
    let chain_id = client.provider.get_chain_id().await?;

    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .with_chain_id(chain_id)
        .connect(&resolved.url)
        .await?;

    let request = alloy_rpc_types::TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(handler)),
        input: TransactionInput::new(calldata),
        ..Default::default()
    };
    let pending = decode_send_transaction(provider.send_transaction(request).await)?;

    let tx_hash = pending.tx_hash();
    println!("sent tx: {tx_hash:#x}");
    Ok(())
}

/// Load a hex string or read hex contents from a file path.
fn load_hex_or_path(value: &str) -> Result<Vec<u8>> {
    if Path::new(value).exists() {
        let contents = fs::read_to_string(value)?;
        return decode_hex(&contents);
    }
    decode_hex(value)
}

/// Decode a hex string, stripping a 0x prefix if present.
fn decode_hex(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex::decode(raw).map_err(|err| anyhow!("invalid hex {value}: {err}"))
}

/// Load a MessageInclusionProof from a JSON string or file path.
fn load_proof(value: &str) -> Result<MessageInclusionProof> {
    if Path::new(value).exists() {
        let contents = fs::read_to_string(value)?;
        return serde_json::from_str(&contents).context("invalid proof json");
    }
    if value.trim_start().starts_with('{') {
        return serde_json::from_str(value).context("invalid proof json");
    }
    anyhow::bail!("proof must be a JSON string or path")
}

/// Decode a revert reason from an error string, if present.
pub fn decode_revert_reason(message: String) -> Option<String> {
    let hex_start = message.find("0x")?;
    let hex_data = &message[hex_start..];
    let hex_end = hex_data.find('"').unwrap_or(hex_data.len());
    let hex_data = &hex_data[..hex_end];
    let data = decode_hex(hex_data).ok()?;
    if data.len() < 4 {
        println!("revert data too short, len={}", data.len());
        return None;
    }
    let selector = &data[..4];
    if selector == [0x08, 0xc3, 0x79, 0xa0] {
        if let Ok(reason) = decode_error_string(&data[4..]) {
            return Some(reason);
        }
    } else if selector == [0x4e, 0x48, 0x7b, 0x71] {
        let code = U256::from_be_slice(&data[4..]);
        return Some(format!("panic({code})"));
    }
    // selector to hex string
    let selector_hex = hex::encode(selector);
    error_selector_map()
        .get(&selector_hex)
        .map(|name| format!("revert: {}", name.to_string()))
        .or_else(|| {
            println!("unknown revert selector 0x{}", selector_hex);
            None
        })
}

/// Decode an ABI-encoded revert string payload.
fn decode_error_string(data: &[u8]) -> Result<String> {
    let value: (String,) = <(String,)>::abi_decode(data)?;
    Ok(value.0)
}

pub fn decode_send_transaction<T>(pending: TransportResult<T>) -> Result<T> {
    let pending = match pending {
        Ok(pending) => pending,
        Err(err) => {
            if let Some(reason) = decode_revert_reason(err.to_string()) {
                return Err(anyhow!("transaction submission reverted: {reason}"));
            } else {
                return Err(anyhow!("transaction submission failed: {err}"));
            }
        }
    };
    Ok(pending)
}
