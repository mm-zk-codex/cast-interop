use crate::abi::{
    bundle_executed_topic, bundle_view, decode_bundle_status, decode_bytes32,
    decode_interop_bundle_sent, encode_bundle_status_call, encode_interop_bundle,
    encode_interop_roots_call, interop_bundle_sent_topic,
};
use crate::cli::BundleTraceArgs;
use crate::config::Config;
use crate::rpc::{get_log_proof, get_logs, get_transaction_receipt, eth_call, RpcClient};
use crate::types::{b256_to_hex, AddressBook};
use alloy_primitives::{B256, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockNumberOrTag, Filter};
use anyhow::{Context, Result};
use serde::Serialize;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceStep {
    step: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceOutput {
    tx_hash: String,
    steps: Vec<TraceStep>,
}

/// Trace the full interop lifecycle for a source transaction.
///
/// Non-blocking: shows a snapshot of the current state across all stages.
pub async fn run(args: BundleTraceArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let source_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;

    let source_client = RpcClient::new(&source_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;

    let mut steps: Vec<TraceStep> = Vec::new();

    // Step 1: Fetch source receipt
    let receipt = match get_transaction_receipt(&source_client, tx_hash).await {
        Ok(r) => {
            steps.push(TraceStep {
                step: "source_receipt".into(),
                status: "ok".into(),
                details: Some(serde_json::json!({
                    "blockNumber": r.block_number,
                    "status": r.status(),
                })),
            });
            r
        }
        Err(err) => {
            steps.push(TraceStep {
                step: "source_receipt".into(),
                status: "error".into(),
                details: Some(serde_json::json!({ "error": err.to_string() })),
            });
            return print_output(&args, tx_hash, steps);
        }
    };

    // Step 2: Decode InteropBundleSent (select Nth matching log via msg_index)
    let mut bundle_data = None;
    let mut match_count = 0u32;
    for log in receipt.logs() {
        if log.topics().first().copied() == Some(interop_bundle_sent_topic()) {
            match decode_interop_bundle_sent(log.data().data.clone()) {
                Ok((_, hash, bundle)) => {
                    if match_count == args.msg_index {
                        bundle_data = Some((hash, bundle));
                        break;
                    }
                    match_count += 1;
                }
                Err(_) => continue,
            }
        }
    }

    let (bundle_hash, bundle) = match bundle_data {
        Some(data) => {
            let view = bundle_view(&data.1);
            steps.push(TraceStep {
                step: "bundle_decoded".into(),
                status: "ok".into(),
                details: Some(serde_json::json!({
                    "bundleHash": b256_to_hex(data.0),
                    "sourceChainId": view.source_chain_id,
                    "destinationChainId": view.destination_chain_id,
                    "callCount": view.calls.len(),
                })),
            });
            data
        }
        None => {
            steps.push(TraceStep {
                step: "bundle_decoded".into(),
                status: "error".into(),
                details: Some(serde_json::json!({
                    "error": "InteropBundleSent event not found in receipt"
                })),
            });
            return print_output(&args, tx_hash, steps);
        }
    };

    // Step 3: Check proof availability (non-blocking)
    let log_proof = match get_log_proof(&source_client, tx_hash, args.msg_index).await {
        Ok(Some(proof)) => {
            steps.push(TraceStep {
                step: "proof_available".into(),
                status: "ok".into(),
                details: Some(serde_json::json!({
                    "batchNumber": proof.batch_number,
                    "root": proof.root,
                    "proofLength": proof.proof.len(),
                })),
            });
            Some(proof)
        }
        Ok(None) => {
            steps.push(TraceStep {
                step: "proof_available".into(),
                status: "pending".into(),
                details: Some(serde_json::json!({
                    "message": "proof not yet available"
                })),
            });
            None
        }
        Err(err) => {
            steps.push(TraceStep {
                step: "proof_available".into(),
                status: "error".into(),
                details: Some(serde_json::json!({ "error": err.to_string() })),
            });
            None
        }
    };

    // Step 4: Check root settlement on dest
    if let Some(ref proof) = log_proof {
        match check_root_settled(&source_client, &dest_client, &addresses, proof).await {
            Ok(step) => steps.push(step),
            Err(err) => steps.push(TraceStep {
                step: "root_settled".into(),
                status: "error".into(),
                details: Some(serde_json::json!({ "error": err.to_string() })),
            }),
        }
    } else {
        steps.push(TraceStep {
            step: "root_settled".into(),
            status: "skipped".into(),
            details: Some(serde_json::json!({
                "message": "skipped (proof not available)"
            })),
        });
    }

    // Step 5: Check bundle status on dest
    let call_data = encode_bundle_status_call(bundle_hash);
    let bundle_status_value =
        match eth_call(&dest_client, addresses.interop_handler, call_data).await {
            Ok(result) => match decode_bundle_status(result) {
                Ok(status) => {
                    let status_str = bundle_status_string(status);
                    steps.push(TraceStep {
                        step: "bundle_status".into(),
                        status: "ok".into(),
                        details: Some(serde_json::json!({
                            "bundleHash": b256_to_hex(bundle_hash),
                            "status": status_str,
                            "statusCode": status,
                        })),
                    });
                    Some(status)
                }
                Err(err) => {
                    steps.push(TraceStep {
                        step: "bundle_status".into(),
                        status: "error".into(),
                        details: Some(serde_json::json!({ "error": err.to_string() })),
                    });
                    None
                }
            },
            Err(err) => {
                steps.push(TraceStep {
                    step: "bundle_status".into(),
                    status: "error".into(),
                    details: Some(serde_json::json!({ "error": err.to_string() })),
                });
                None
            }
        };

    // Step 6: Scan for BundleExecuted event on dest (only if FullyExecuted)
    let is_executed = bundle_status_value == Some(2);
    if is_executed {
        match dest_client.provider.get_block_number().await {
            Ok(latest_block) => {
                let from_block = latest_block.saturating_sub(args.scan_blocks);
                let filter = Filter::new()
                    .from_block(BlockNumberOrTag::Number(from_block))
                    .to_block(BlockNumberOrTag::Latest)
                    .event_signature(bundle_executed_topic())
                    .topic1(bundle_hash);

                match get_logs(&dest_client, filter).await {
                    Ok(logs) => {
                        if let Some(log) = logs.first() {
                            steps.push(TraceStep {
                                step: "execution_tx".into(),
                                status: "ok".into(),
                                details: Some(serde_json::json!({
                                    "txHash": log.transaction_hash.map(|h| format!("{h:#x}")),
                                    "blockNumber": log.block_number,
                                })),
                            });
                        } else {
                            steps.push(TraceStep {
                                step: "execution_tx".into(),
                                status: "not_found".into(),
                                details: Some(serde_json::json!({
                                    "message": format!("BundleExecuted event not found in last {} blocks", args.scan_blocks),
                                })),
                            });
                        }
                    }
                    Err(err) => {
                        steps.push(TraceStep {
                            step: "execution_tx".into(),
                            status: "error".into(),
                            details: Some(serde_json::json!({ "error": err.to_string() })),
                        });
                    }
                }
            }
            Err(err) => {
                steps.push(TraceStep {
                    step: "execution_tx".into(),
                    status: "error".into(),
                    details: Some(serde_json::json!({
                        "error": format!("failed to get dest block number: {err}")
                    })),
                });
            }
        }
    } else {
        steps.push(TraceStep {
            step: "execution_tx".into(),
            status: "skipped".into(),
            details: Some(serde_json::json!({
                "message": "bundle not yet fully executed"
            })),
        });
    }

    // Suppress unused variable warning — bundle is needed for decode step
    let _encoded = encode_interop_bundle(&bundle);

    print_output(&args, tx_hash, steps)
}

fn print_output(args: &BundleTraceArgs, tx_hash: B256, steps: Vec<TraceStep>) -> Result<()> {
    let output = TraceOutput {
        tx_hash: format!("{tx_hash:#x}"),
        steps: steps.clone(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("trace for tx {tx_hash:#x}\n");
        for step in &steps {
            let icon = match step.status.as_str() {
                "ok" => "+",
                "pending" => "~",
                "skipped" => "-",
                "error" => "!",
                "not_found" => "?",
                _ => " ",
            };
            print!("[{icon}] {}: {}", step.step, step.status);
            if let Some(details) = &step.details {
                if let Some(obj) = details.as_object() {
                    for (k, v) in obj {
                        let val = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        print!("  {k}={val}");
                    }
                }
            }
            println!();
        }
    }
    Ok(())
}

async fn check_root_settled(
    source_client: &RpcClient,
    dest_client: &RpcClient,
    addresses: &AddressBook,
    proof: &crate::rpc::LogProof,
) -> Result<TraceStep> {
    let source_chain_id = source_client
        .provider
        .get_chain_id()
        .await
        .context("failed to get source chain ID")?;
    let data = encode_interop_roots_call(
        U256::from(source_chain_id),
        U256::from(proof.batch_number),
    );
    let result = eth_call(dest_client, addresses.interop_root_storage, data).await?;
    let root = decode_bytes32(result)?;
    let expected = &proof.root;
    let root_hex = b256_to_hex(root);
    if root == B256::ZERO {
        Ok(TraceStep {
            step: "root_settled".into(),
            status: "pending".into(),
            details: Some(serde_json::json!({
                "message": "root not yet available on destination",
                "batchNumber": proof.batch_number,
            })),
        })
    } else if root_hex == *expected {
        Ok(TraceStep {
            step: "root_settled".into(),
            status: "ok".into(),
            details: Some(serde_json::json!({
                "root": root_hex,
                "batchNumber": proof.batch_number,
            })),
        })
    } else {
        Ok(TraceStep {
            step: "root_settled".into(),
            status: "error".into(),
            details: Some(serde_json::json!({
                "message": "root mismatch",
                "expected": expected,
                "actual": root_hex,
            })),
        })
    }
}

fn bundle_status_string(value: u8) -> String {
    match value {
        0 => "Unreceived",
        1 => "Verified",
        2 => "FullyExecuted",
        3 => "Unbundled",
        _ => "Unknown",
    }
    .to_string()
}
