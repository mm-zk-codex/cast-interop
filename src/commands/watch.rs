use crate::abi::{decode_bundle_status, encode_bundle_status_call, encode_interop_roots_call};
use crate::cli::WatchArgs;
use crate::config::Config;
use crate::rpc::{
    eth_call, get_finalized_block_number, get_log_proof, get_transaction_receipt, RpcClient,
};
use crate::types::{parse_b256, AddressBook};
use alloy_primitives::{B256, U256};
use alloy_provider::Provider;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WatchEvent {
    event: String,
    details: serde_json::Value,
}

pub async fn run(args: WatchArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let src_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;
    let source_client = RpcClient::new(&src_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let tx_hash = parse_b256(&args.tx)?;
    let receipt = get_transaction_receipt(&source_client, tx_hash).await?;
    let block_number = receipt
        .block_number
        .ok_or_else(|| anyhow!("missing receipt block number"))?;

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll = Duration::from_millis(args.poll_ms.unwrap_or(1_000));
    let start = tokio::time::Instant::now();

    let mut finalized = false;
    let mut log_proof = None;
    let mut root_available = false;
    let bundle_hash = extract_bundle_hash(&receipt)?;
    let mut bundle_status: Option<u8> = None;

    loop {
        if !finalized {
            let finalized_block = get_finalized_block_number(&source_client).await;
            if let Ok(finalized_block) = finalized_block {
                if finalized_block >= block_number {
                    finalized = true;
                    emit_event(
                        args.json,
                        "finalized",
                        serde_json::json!({ "block": finalized_block }),
                    );
                }
            }
        }

        if log_proof.is_none() {
            if let Some(proof) = get_log_proof(&source_client, tx_hash, args.msg_index).await? {
                emit_event(
                    args.json,
                    "log_proof",
                    serde_json::json!({
                        "batch": proof.batch_number,
                        "id": proof.id,
                        "root": proof.root,
                    }),
                );
                log_proof = Some(proof);
            }
        }

        if let Some(proof) = log_proof.as_ref() {
            if !root_available {
                let root = fetch_root(
                    &dest_client,
                    addresses.interop_root_storage,
                    proof.batch_number,
                    &proof.root,
                    &source_client,
                )
                .await?;
                if root {
                    root_available = true;
                    emit_event(
                        args.json,
                        "root_available",
                        serde_json::json!({ "root": proof.root, "batch": proof.batch_number }),
                    );
                }
            }
        }

        if let Some(hash) = bundle_hash {
            let status = fetch_bundle_status(&dest_client, addresses.interop_handler, hash).await?;
            if bundle_status != Some(status) {
                bundle_status = Some(status);
                emit_event(
                    args.json,
                    "bundle_status",
                    serde_json::json!({ "bundleHash": format!("{hash:#x}"), "status": bundle_status_string(status) }),
                );
            }
        }

        if let Some(target) = args.until.as_deref() {
            if target == "verified" {
                if matches!(bundle_status, Some(1 | 2)) {
                    return Ok(());
                }
            } else if target == "executed" {
                if matches!(bundle_status, Some(2)) {
                    return Ok(());
                }
            } else {
                anyhow::bail!("invalid --until value {target} (expected verified or executed)");
            }
        }

        if start.elapsed() > timeout {
            anyhow::bail!("watch timeout reached");
        }
        tokio::time::sleep(poll).await;
    }
}

fn emit_event(json: bool, name: &str, details: serde_json::Value) {
    if json {
        let event = WatchEvent {
            event: name.to_string(),
            details,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&event).unwrap_or_default()
        );
    } else {
        println!("{name}: {details}");
    }
}

async fn fetch_root(
    dest_client: &RpcClient,
    root_storage: alloy_primitives::Address,
    batch_number: u64,
    expected_root: &str,
    source_client: &RpcClient,
) -> Result<bool> {
    let source_chain_id = source_client.provider.get_chain_id().await?;
    let data = encode_interop_roots_call(U256::from(source_chain_id), U256::from(batch_number));
    let result = eth_call(dest_client, root_storage, data).await?;
    let root = crate::abi::decode_bytes32(result)?;
    Ok(root != B256::ZERO && format!("{root:#x}").eq_ignore_ascii_case(expected_root))
}

async fn fetch_bundle_status(
    client: &RpcClient,
    handler: alloy_primitives::Address,
    bundle_hash: B256,
) -> Result<u8> {
    let call = encode_bundle_status_call(bundle_hash);
    let data = eth_call(client, handler, call).await?;
    decode_bundle_status(data)
}

fn extract_bundle_hash(receipt: &alloy_rpc_types::TransactionReceipt) -> Result<Option<B256>> {
    for log in receipt.logs() {
        if log.topics().first().copied() == Some(crate::abi::interop_bundle_sent_topic()) {
            let (_, hash, _) = crate::abi::decode_interop_bundle_sent(log.data().data.clone())
                .context("failed to decode InteropBundleSent")?;
            return Ok(Some(hash));
        }
    }
    Ok(None)
}

fn bundle_status_string(value: u8) -> &'static str {
    match value {
        0 => "Unreceived",
        1 => "Verified",
        2 => "FullyExecuted",
        3 => "Unbundled",
        _ => "Unknown",
    }
}
