use crate::abi::{
    decode_bytes32, decode_interop_bundle_sent, encode_interop_bundle, encode_send_message_call,
    interop_bundle_sent_topic,
};
use crate::cli::SendBatchArgs;
use crate::commands::bundle_action::decode_send_transaction;
use crate::config::Config;
use crate::encode::{
    encode_evm_v1_with_address, encode_execution_address, encode_indirect_call,
    encode_interop_call_value, encode_unbundler_address, parse_permissionless_address,
};
use crate::relay_flow::{build_message_proof, execute_bundle, wait_for_proof, wait_for_root};
use crate::rpc::{eth_call_with_value, RpcClient};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{
    bytes_from_hex, parse_address, parse_u256, require_signer_or_dry_run, AddressBook,
};
use alloy_dyn_abi::{JsonAbiExt, Specifier};
use alloy_json_abi::Function;
use alloy_primitives::{Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{TransactionInput, TransactionRequest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct BatchFile {
    messages: Vec<BatchMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchMessage {
    to_chain: String,
    to: String,
    payload: Option<String>,
    abi: Option<String>,
    args: Option<Vec<serde_json::Value>>,
    interop_value: Option<String>,
    indirect: Option<String>,
    execution_address: Option<String>,
    unbundler: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResultEntry {
    index: usize,
    tx_hash: Option<String>,
    status: bool,
    send_id: Option<String>,
    relay_tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchOutput {
    total: usize,
    sent: usize,
    relayed: usize,
    results: Vec<BatchResultEntry>,
}

/// Send multiple interop messages from a JSON batch file.
///
/// Optionally relays each message after sending.
pub async fn run(args: SendBatchArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;

    let contents =
        fs::read_to_string(&args.file).context("failed to read batch file")?;
    let batch: BatchFile = serde_json::from_str(&contents).context("invalid batch JSON")?;
    if batch.messages.is_empty() {
        anyhow::bail!("batch file must include at least one message");
    }

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;
    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "send batch")?;

    let dest_client = if args.relay {
        let dest_rpc =
            config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;
        Some(RpcClient::new(&dest_rpc.url).await?)
    } else {
        None
    };

    let dest_rpc_url = if args.relay {
        let dest_rpc =
            config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;
        Some(dest_rpc.url.clone())
    } else {
        None
    };

    let total = batch.messages.len();
    let mut results: Vec<BatchResultEntry> = Vec::new();
    let mut sent_count = 0usize;
    let mut relayed_count = 0usize;

    for (idx, msg) in batch.messages.iter().enumerate() {
        let result = send_one_message(
            &args,
            &config,
            &client,
            &resolved.url,
            &addresses,
            wallet.as_ref(),
            msg,
            idx,
        )
        .await;

        match result {
            Ok(SendResult::DryRun { send_id }) => {
                sent_count += 1;
                let entry = BatchResultEntry {
                    index: idx,
                    tx_hash: None,
                    status: true,
                    send_id: Some(format!("{send_id:#x}")),
                    relay_tx_hash: None,
                    error: None,
                };
                if !args.json {
                    println!(
                        "[{}/{}] dry-run sendId: {send_id:#x}",
                        idx + 1,
                        total
                    );
                }
                results.push(entry);
            }
            Ok(SendResult::Sent {
                tx_hash,
                send_id,
                receipt,
            }) => {
                let tx_status = receipt.status();
                if tx_status {
                    sent_count += 1;
                }
                let mut entry = BatchResultEntry {
                    index: idx,
                    tx_hash: Some(format!("{tx_hash:#x}")),
                    status: tx_status,
                    send_id: send_id.map(|id| format!("{id:#x}")),
                    relay_tx_hash: None,
                    error: if tx_status {
                        None
                    } else {
                        Some("transaction reverted".into())
                    },
                };

                if args.relay && tx_status {
                    if let (Some(dest_client), Some(dest_url)) =
                        (&dest_client, &dest_rpc_url)
                    {
                        match relay_message(
                            &args,
                            &client,
                            dest_client,
                            dest_url,
                            &addresses,
                            wallet.as_ref().expect("wallet required for relay"),
                            tx_hash,
                            &receipt,
                            idx,
                        )
                        .await
                        {
                            Ok(relay_hash) => {
                                relayed_count += 1;
                                entry.relay_tx_hash =
                                    Some(format!("{relay_hash:#x}"));
                            }
                            Err(err) => {
                                entry.error =
                                    Some(format!("relay failed: {err}"));
                            }
                        }
                    }
                }

                if !args.json {
                    print!("[{}/{}] tx: {tx_hash:#x}", idx + 1, total);
                    if let Some(ref relay_hash) = entry.relay_tx_hash {
                        print!("  relayed: {relay_hash}");
                    }
                    if let Some(ref err) = entry.error {
                        print!("  error: {err}");
                    }
                    println!();
                }
                results.push(entry);
            }
            Err(err) => {
                if !args.json {
                    println!("[{}/{}] error: {err}", idx + 1, total);
                }
                results.push(BatchResultEntry {
                    index: idx,
                    tx_hash: None,
                    status: false,
                    send_id: None,
                    relay_tx_hash: None,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    let output = BatchOutput {
        total,
        sent: sent_count,
        relayed: relayed_count,
        results,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "\nbatch complete: {sent_count}/{total} sent, {relayed_count} relayed"
        );
    }

    let failed = output.results.iter().any(|r| !r.status || r.error.is_some());
    if failed {
        let fail_count = output
            .results
            .iter()
            .filter(|r| !r.status || r.error.is_some())
            .count();
        anyhow::bail!("{fail_count}/{total} batch operations failed");
    }

    Ok(())
}

enum SendResult {
    Sent {
        tx_hash: B256,
        send_id: Option<B256>,
        receipt: alloy_rpc_types::TransactionReceipt,
    },
    DryRun {
        send_id: B256,
    },
}

async fn send_one_message(
    args: &SendBatchArgs,
    config: &Config,
    client: &RpcClient,
    rpc_url: &str,
    addresses: &AddressBook,
    wallet: Option<&alloy_signer_local::PrivateKeySigner>,
    msg: &BatchMessage,
    _idx: usize,
) -> Result<SendResult> {
    let dest_chain_id = config.resolve_chain_id(&msg.to_chain)?;
    let to = parse_address(&msg.to)?;

    let payload = resolve_payload(msg)?;
    let attributes = build_attributes(msg, dest_chain_id)?;
    let msg_value = compute_value(msg)?;

    let recipient = encode_evm_v1_with_address(dest_chain_id, to);
    let calldata = encode_send_message_call(recipient, payload, attributes)?;

    if args.dry_run {
        let result = eth_call_with_value(
            client,
            addresses.interop_center,
            calldata,
            Some(msg_value),
        )
        .await?;
        let send_id = decode_bytes32(result)?;
        return Ok(SendResult::DryRun { send_id });
    }

    let wallet = wallet.ok_or_else(|| anyhow::anyhow!("wallet required"))?;
    let chain_id = client.provider.get_chain_id().await?;
    let provider = ProviderBuilder::new()
        .wallet(wallet.clone())
        .with_chain_id(chain_id)
        .connect(rpc_url)
        .await?;

    let request = TransactionRequest {
        to: Some(addresses.interop_center.into()),
        input: TransactionInput::new(calldata),
        value: Some(msg_value),
        ..Default::default()
    };

    let pending = decode_send_transaction(provider.send_transaction(request).await)?;
    let tx_hash = *pending.tx_hash();
    let receipt = pending.get_receipt().await?;

    let send_id = extract_send_id(receipt.logs(), addresses.interop_center);

    Ok(SendResult::Sent {
        tx_hash,
        send_id,
        receipt,
    })
}

async fn relay_message(
    args: &SendBatchArgs,
    source_client: &RpcClient,
    dest_client: &RpcClient,
    dest_rpc_url: &str,
    addresses: &AddressBook,
    wallet: &alloy_signer_local::PrivateKeySigner,
    tx_hash: B256,
    receipt: &alloy_rpc_types::TransactionReceipt,
    _idx: usize,
) -> Result<B256> {
    // Find the InteropBundleSent event
    let mut bundle = None;
    for log in receipt.logs() {
        if log.topics().first().copied() == Some(interop_bundle_sent_topic()) {
            let (_, _hash, interop_bundle) =
                decode_interop_bundle_sent(log.data().data.clone())?;
            bundle = Some(interop_bundle);
            break;
        }
    }
    let bundle =
        bundle.ok_or_else(|| anyhow::anyhow!("InteropBundleSent not found in receipt"))?;
    let encoded_bundle = encode_interop_bundle(&bundle);

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll_ms = args.poll_ms.unwrap_or(1_000);

    let log_proof = wait_for_proof(
        source_client,
        receipt.block_number.context("missing block number")?,
        tx_hash,
        0,
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let source_chain_id = source_client.provider.get_chain_id().await?;

    wait_for_root(
        dest_client,
        addresses.interop_root_storage,
        source_chain_id,
        log_proof.batch_number,
        log_proof.root.clone(),
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let proof = build_message_proof(
        &log_proof,
        receipt
            .transaction_index
            .context("missing transaction index")?,
        addresses.interop_center,
        &encoded_bundle,
        source_chain_id,
    );

    execute_bundle(
        dest_client,
        dest_rpc_url,
        addresses.interop_handler,
        wallet.clone(),
        encoded_bundle,
        proof,
    )
    .await
}

fn resolve_payload(msg: &BatchMessage) -> Result<Bytes> {
    match (&msg.payload, &msg.abi) {
        (Some(hex), None) => bytes_from_hex(hex),
        (None, Some(sig)) => {
            let args = msg
                .args
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("abi requires args"))?;
            abi_encode_call(sig, args)
        }
        (Some(_), Some(_)) => anyhow::bail!("cannot set both payload and abi"),
        (None, None) => anyhow::bail!("message requires payload or abi+args"),
    }
}

fn abi_encode_call(sig: &str, args: &[serde_json::Value]) -> Result<Bytes> {
    let func = Function::parse(sig).context("invalid ABI signature")?;
    if args.len() != func.inputs.len() {
        anyhow::bail!(
            "ABI argument count mismatch for {}: expected {}, got {}",
            sig,
            func.inputs.len(),
            args.len()
        );
    }
    let tokens = func
        .inputs
        .iter()
        .zip(args.iter())
        .map(|(input, val)| {
            let ty = input.resolve().context("cannot resolve ABI type")?;
            let json_str = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            ty.coerce_str(&json_str)
                .context(format!("cannot parse arg for type {ty}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let encoded = func.abi_encode_input(&tokens)?;
    Ok(Bytes::from(encoded))
}

fn build_attributes(msg: &BatchMessage, dest_chain_id: U256) -> Result<Vec<Bytes>> {
    let mut attributes: Vec<Bytes> = Vec::new();
    if let Some(value) = msg.interop_value.as_deref() {
        attributes.push(encode_interop_call_value(parse_u256(value)?));
    }
    if let Some(value) = msg.indirect.as_deref() {
        attributes.push(encode_indirect_call(parse_u256(value)?));
    }
    if let Some(value) = msg.execution_address.as_deref() {
        let encoded = match parse_permissionless_address(value)? {
            None => Bytes::new(),
            Some(addr) => encode_evm_v1_with_address(dest_chain_id, addr),
        };
        attributes.push(encode_execution_address(encoded));
    }
    if let Some(value) = msg.unbundler.as_deref() {
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

fn compute_value(msg: &BatchMessage) -> Result<U256> {
    let mut total = U256::ZERO;
    if let Some(value) = msg.interop_value.as_deref() {
        total += parse_u256(value)?;
    }
    if let Some(value) = msg.indirect.as_deref() {
        total += parse_u256(value)?;
    }
    Ok(total)
}

fn extract_send_id(
    logs: &[alloy_rpc_types::Log],
    center: alloy_primitives::Address,
) -> Option<B256> {
    for log in logs {
        if log.address() == center
            && log.topics().first().copied() == Some(crate::abi::message_sent_topic())
        {
            return log.topics().get(1).copied();
        }
    }
    None
}
