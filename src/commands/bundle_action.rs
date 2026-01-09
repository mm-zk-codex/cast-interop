use crate::abi::{encode_execute_bundle_call, encode_verify_bundle_call, error_selector_map};
use crate::cli::BundleActionArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{
    require_signer_or_dry_run, AddressBook, MessageInclusionProof, BUNDLE_IDENTIFIER,
};
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionInput;
use alloy_sol_types::SolValue;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::str::FromStr;

use alloy_signer_local::PrivateKeySigner;

pub async fn run_verify(
    args: BundleActionArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    run_bundle_action("bundle verify", args, config, addresses, true).await
}

pub async fn run_execute(
    args: BundleActionArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    run_bundle_action("bundle execute", args, config, addresses, false).await
}

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

    if args.private_key.is_some() && args.private_key_env.is_some() {
        anyhow::bail!("cannot set both --private-key and --private-key-env");
    }

    let private_key_env = args
        .private_key_env
        .clone()
        .unwrap_or_else(|| config.signer_env());

    let wallet = if let Some(key) = args.private_key.clone() {
        Some(load_wallet(&key)?)
    } else if let Ok(key) = std::env::var(private_key_env) {
        Some(load_wallet(&key)?)
    } else {
        None
    };

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

    let client = RpcClient::new(&args.rpc).await?;
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
        .connect(&args.rpc)
        .await?;

    let request = alloy_rpc_types::TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(handler)),
        input: TransactionInput::new(calldata),
        ..Default::default()
    };
    let pending = provider.send_transaction(request).await?;
    let tx_hash = pending.tx_hash();
    println!("sent tx: {tx_hash:#x}");
    Ok(())
}

fn load_wallet(key: &str) -> Result<PrivateKeySigner> {
    let pk_signer: PrivateKeySigner = key.parse()?;
    Ok(pk_signer)
}

fn load_hex_or_path(value: &str) -> Result<Vec<u8>> {
    if Path::new(value).exists() {
        let contents = fs::read_to_string(value)?;
        return decode_hex(&contents);
    }
    decode_hex(value)
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex::decode(raw).map_err(|err| anyhow!("invalid hex {value}: {err}"))
}

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

fn decode_revert_reason(message: String) -> Option<String> {
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

fn decode_error_string(data: &[u8]) -> Result<String> {
    let value: (String,) = <(String,)>::abi_decode(data)?;
    Ok(value.0)
}
