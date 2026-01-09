use crate::abi::{decode_bytes32, encode_send_bundle_call, encode_send_message_call};
use crate::cli::{SendBundleArgs, SendMessageArgs};
use crate::config::Config;
use crate::encode::{
    encode_evm_v1_address_only, encode_evm_v1_chain_only, encode_evm_v1_with_address,
    encode_execution_address, encode_indirect_call, encode_interop_call_value,
    encode_unbundler_address, parse_payload, parse_permissionless_address,
};
use crate::rpc::{eth_call_with_value, RpcClient};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{parse_address, parse_u256, require_signer_or_dry_run, AddressBook};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{TransactionInput, TransactionRequest};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde::Serialize;
use std::fs;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendOutput {
    tx_hash: String,
    status: Option<u64>,
    send_id: Option<String>,
    bundle_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallFile {
    calls: Vec<CallEntry>,
}

#[derive(Debug, Deserialize)]
struct CallEntry {
    to: String,
    data: String,
    attributes: Option<CallAttributesEntry>,
}

#[derive(Debug, Deserialize)]
struct CallAttributesEntry {
    #[serde(rename = "interopValue")]
    interop_value: Option<String>,
    indirect: Option<String>,
}

pub async fn run_message(
    args: SendMessageArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let dest_chain_id = config.resolve_chain_id(&args.to_chain)?;
    let to = parse_address(&args.to)?;
    let payload = parse_payload(args.payload.as_deref(), args.payload_file.as_deref())?;

    let attributes = build_message_attributes(&args, dest_chain_id)?;
    let msg_value = message_value(&args)?;
    let recipient = encode_evm_v1_with_address(dest_chain_id, to);
    let calldata = encode_send_message_call(recipient, payload, attributes.clone())?;

    let client = RpcClient::new(&resolved.url).await?;

    if args.dry_run {
        let result = eth_call_with_value(
            &client,
            addresses.interop_center,
            calldata.clone(),
            Some(msg_value),
        )
        .await?;
        let send_id = decode_bytes32(result)?;
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "dryRun": true,
                    "sendId": format!("{send_id:#x}")
                }))?
            );
        } else {
            println!("dry-run sendId: {send_id:#x}");
        }
        return Ok(());
    }

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;
    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "send message")?;

    let wallet = wallet.expect("wallet required");
    let chain_id = client.provider.get_chain_id().await?;
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .with_chain_id(chain_id)
        .connect(&resolved.url)
        .await?;

    let request = TransactionRequest {
        to: Some(addresses.interop_center.into()),
        input: TransactionInput::new(calldata),
        value: Some(msg_value),
        ..Default::default()
    };
    let pending = provider.send_transaction(request).await?;
    let tx_hash = pending.tx_hash();
    let receipt = pending.get_receipt().await?;

    let send_id = extract_send_id(receipt.logs(), addresses.interop_center);

    let output = SendOutput {
        tx_hash: format!("{tx_hash:#x}"),
        status: receipt.status,
        send_id: send_id.map(|id| format!("{id:#x}")),
        bundle_hash: None,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("tx hash: {}", output.tx_hash);
        println!(
            "status: {}",
            output
                .status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        if let Some(send_id) = output.send_id {
            println!("sendId: {send_id}");
        }
    }

    Ok(())
}

pub async fn run_bundle(
    args: SendBundleArgs,
    config: Config,
    addresses: AddressBook,
) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let dest_chain_id = config.resolve_chain_id(&args.to_chain)?;
    let file = load_calls(&args.calls)?;

    let (call_starters, total_value) = build_call_starters(&file.calls)?;
    let bundle_attributes = build_bundle_attributes(&args, dest_chain_id)?;
    let destination_chain = encode_evm_v1_chain_only(dest_chain_id);
    let calldata = encode_send_bundle_call(destination_chain, call_starters, bundle_attributes)?;

    let client = RpcClient::new(&resolved.url).await?;
    if args.dry_run {
        let result = eth_call_with_value(
            &client,
            addresses.interop_center,
            calldata.clone(),
            Some(total_value),
        )
        .await?;
        let bundle_hash = decode_bytes32(result)?;
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "dryRun": true,
                    "bundleHash": format!("{bundle_hash:#x}")
                }))?
            );
        } else {
            println!("dry-run bundleHash: {bundle_hash:#x}");
        }
        return Ok(());
    }

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;
    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "send bundle")?;

    let wallet = wallet.expect("wallet required");
    let chain_id = client.provider.get_chain_id().await?;
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .with_chain_id(chain_id)
        .connect(&resolved.url)
        .await?;

    let request = TransactionRequest {
        to: Some(addresses.interop_center.into()),
        input: TransactionInput::new(calldata),
        value: Some(total_value),
        ..Default::default()
    };
    let pending = provider.send_transaction(request).await?;
    let tx_hash = pending.tx_hash();
    let receipt = pending.get_receipt().await?;

    let bundle_hash = extract_bundle_hash(receipt.logs(), addresses.interop_center);

    let output = SendOutput {
        tx_hash: format!("{tx_hash:#x}"),
        status: receipt.status,
        send_id: None,
        bundle_hash: bundle_hash.map(|hash| format!("{hash:#x}")),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("tx hash: {}", output.tx_hash);
        println!(
            "status: {}",
            output
                .status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        if let Some(bundle_hash) = output.bundle_hash {
            println!("bundleHash: {bundle_hash}");
        }
    }
    Ok(())
}

fn build_message_attributes(args: &SendMessageArgs, dest_chain_id: U256) -> Result<Vec<Bytes>> {
    let mut attributes: Vec<Bytes> = Vec::new();
    if let Some(value) = args.interop_value.as_deref() {
        attributes.push(encode_interop_call_value(parse_u256(value)?));
    }
    if let Some(value) = args.indirect.as_deref() {
        attributes.push(encode_indirect_call(parse_u256(value)?));
    }
    if let Some(value) = args.execution_address.as_deref() {
        let encoded = match parse_permissionless_address(value)? {
            None => Bytes::new(),
            Some(addr) => encode_evm_v1_with_address(dest_chain_id, addr),
        };
        attributes.push(encode_execution_address(encoded));
    }
    if let Some(value) = args.unbundler.as_deref() {
        if value == "permissionless" {
            anyhow::bail!("unbundler cannot be permissionless");
        }
        let addr = parse_address(value)?;
        attributes.push(encode_unbundler_address(encode_evm_v1_with_address(
            dest_chain_id,
            addr,
        )));
    }
    Ok(attributes)
}

fn message_value(args: &SendMessageArgs) -> Result<U256> {
    let mut total = U256::ZERO;
    if let Some(value) = args.interop_value.as_deref() {
        total += parse_u256(value)?;
    }
    if let Some(value) = args.indirect.as_deref() {
        total += parse_u256(value)?;
    }
    Ok(total)
}

fn build_call_starters(calls: &[CallEntry]) -> Result<(Vec<crate::abi::InteropCallStarter>, U256)> {
    let mut starters = Vec::new();
    let mut total_value = U256::ZERO;

    for call in calls {
        let to = parse_address(&call.to)?;
        let data = crate::types::bytes_from_hex(&call.data)?;
        let (attributes, value) = build_call_attributes(call.attributes.as_ref())?;
        total_value += value;
        starters.push(crate::abi::InteropCallStarter {
            to: encode_evm_v1_address_only(to),
            data,
            callAttributes: attributes,
        });
    }
    Ok((starters, total_value))
}

fn build_call_attributes(
    attributes: Option<&CallAttributesEntry>,
) -> Result<(Vec<Bytes>, U256)> {
    let mut output = Vec::new();
    let mut value = U256::ZERO;

    if let Some(attributes) = attributes {
        if let Some(interop_value) = attributes.interop_value.as_deref() {
            let parsed = parse_u256(interop_value)?;
            value += parsed;
            output.push(encode_interop_call_value(parsed));
        }
        if let Some(indirect) = attributes.indirect.as_deref() {
            let parsed = parse_u256(indirect)?;
            value += parsed;
            output.push(encode_indirect_call(parsed));
        }
    }

    Ok((output, value))
}

fn build_bundle_attributes(args: &SendBundleArgs, dest_chain_id: U256) -> Result<Vec<Bytes>> {
    let mut attributes = Vec::new();

    if let Some(value) = args.bundle_execution_address.as_deref() {
        let encoded = match parse_permissionless_address(value)? {
            None => Bytes::new(),
            Some(addr) => encode_evm_v1_with_address(dest_chain_id, addr),
        };
        attributes.push(encode_execution_address(encoded));
    }
    if let Some(value) = args.bundle_unbundler.as_deref() {
        if value == "permissionless" {
            anyhow::bail!("bundle unbundler cannot be permissionless");
        }
        let addr = parse_address(value)?;
        attributes.push(encode_unbundler_address(encode_evm_v1_with_address(
            dest_chain_id,
            addr,
        )));
    }

    Ok(attributes)
}

fn load_calls(path: &std::path::Path) -> Result<CallFile> {
    let contents = fs::read_to_string(path).context("failed to read calls.json")?;
    let file: CallFile = serde_json::from_str(&contents).context("invalid calls.json")?;
    if file.calls.is_empty() {
        anyhow::bail!("calls.json must include at least one call");
    }
    Ok(file)
}

fn extract_send_id(logs: &[alloy_rpc_types::Log], center: Address) -> Option<B256> {
    for log in logs {
        if log.address() == center && log.topics().first().copied() == Some(crate::abi::message_sent_topic()) {
            return log.topics().get(1).copied();
        }
    }
    None
}

fn extract_bundle_hash(logs: &[alloy_rpc_types::Log], center: Address) -> Option<B256> {
    for log in logs {
        if log.address() == center
            && log.topics().first().copied() == Some(crate::abi::interop_bundle_sent_topic())
        {
            if let Ok((_, bundle_hash, _)) =
                crate::abi::decode_interop_bundle_sent(log.data().data.clone())
            {
                return Some(bundle_hash);
            }
        }
    }
    None
}
