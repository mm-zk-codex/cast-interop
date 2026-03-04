use crate::abi::{
    decode_interop_bundle_sent, encode_execute_bundle_call, encode_interop_bundle,
    encode_verify_bundle_call, interop_bundle_sent_topic, l1_message_sent_topic,
};
use crate::cli::RelayArgs;
use crate::commands::bundle_action::decode_send_transaction;
use crate::config::Config;
use crate::relay_flow::{build_message_proof, wait_for_proof, wait_for_root};
use crate::rpc::{eth_call, get_transaction_receipt, RpcClient};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{
    format_hex, require_signer_or_dry_run, AddressBook, MessageInclusionProof, RelaySummary,
    L1_SENDER_ADDRESS,
};
use alloy_primitives::{Address, Bytes, B256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{Log, TransactionReceipt};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

/// A bundle detected within a transaction receipt, with its computed msg_index.
#[derive(Debug, Clone)]
pub struct DetectedBundle {
    pub bundle_hash: B256,
    pub encoded_bundle: Bytes,
    /// The L2-to-L1 message index used to fetch the proof for this bundle.
    pub msg_index: u32,
}

/// Scan a log sequence to locate each `InteropBundleSent` log from `center` and
/// compute its `msg_index` by counting `L1MessageSent` events from `L1_SENDER_ADDRESS`
/// (0x8008) in log order.
///
/// Returns `(log_position, msg_index)` pairs for every detected bundle log.
/// This is the core counting logic and is separately unit-testable.
pub fn find_bundle_log_positions(logs: &[Log], center: Address) -> Vec<(usize, u32)> {
    let bundle_topic = interop_bundle_sent_topic();
    let l1_msg_topic = l1_message_sent_topic();
    let l1_messenger = L1_SENDER_ADDRESS;

    let mut l1_count: u32 = 0;
    let mut results = Vec::new();

    for (idx, log) in logs.iter().enumerate() {
        let topic = log.topics().first().copied();

        if log.address() == l1_messenger && topic == Some(l1_msg_topic) {
            l1_count += 1;
        }

        if log.address() == center && topic == Some(bundle_topic) {
            // msg_index is 0-based; the bundle's own L1MessageSent has already been counted.
            results.push((idx, l1_count.saturating_sub(1)));
        }
    }
    results
}

/// Extract all interop bundles from a receipt, with correct msg_index per bundle.
///
/// Each `InteropBundleSent` emitted by `center` gets a `msg_index` derived by
/// counting `L1MessageSent` events from the L2→L1 messenger (0x8008) in log
/// order — the same approach used by `auto-relay`.
pub fn extract_bundles(
    receipt: &TransactionReceipt,
    center: Address,
) -> Result<Vec<DetectedBundle>> {
    let logs: Vec<Log> = receipt.inner.logs().to_vec();
    let positions = find_bundle_log_positions(&logs, center);

    let mut bundles = Vec::new();
    for (log_idx, msg_index) in positions {
        let log = &logs[log_idx];
        let (_, bundle_hash, interop_bundle) =
            decode_interop_bundle_sent(log.data().data.clone())
                .context("failed to decode InteropBundleSent")?;
        let encoded_bundle = encode_interop_bundle(&interop_bundle);
        bundles.push(DetectedBundle {
            bundle_hash,
            encoded_bundle,
            msg_index,
        });
    }

    Ok(bundles)
}

/// Relay a bundle end-to-end across chains.
///
/// With `--all`, relays every bundle emitted by the transaction in order.
/// Without `--all`, relays the single bundle whose msg_index matches `--msg-index`
/// (default 0).
pub async fn run(args: RelayArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let handler = args
        .handler
        .as_deref()
        .map(Address::from_str)
        .transpose()
        .context("invalid handler address")?
        .unwrap_or(addresses.interop_handler);
    let center = args
        .center
        .as_deref()
        .map(Address::from_str)
        .transpose()
        .context("invalid center address")?
        .unwrap_or(addresses.interop_center);
    let root_storage = args
        .root_storage
        .as_deref()
        .map(Address::from_str)
        .transpose()
        .context("invalid root storage address")?
        .unwrap_or(addresses.interop_root_storage);

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;

    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "relay")?;

    let source_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;

    let source_client = RpcClient::new(&source_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&source_client, tx_hash).await?;

    let all_bundles = extract_bundles(&receipt, center)?;
    if all_bundles.is_empty() {
        anyhow::bail!("no InteropBundleSent events found in transaction {}", args.tx);
    }

    let to_relay: Vec<DetectedBundle> = if args.all {
        println!(
            "relaying {} bundle(s) from transaction {tx_hash:#x}",
            all_bundles.len()
        );
        all_bundles
    } else {
        let target = args.msg_index;
        let selected = all_bundles
            .into_iter()
            .find(|b| b.msg_index == target)
            .ok_or_else(|| {
                anyhow!(
                    "no bundle with msg_index={target} in tx {tx_hash:#x} \
                     (use --all to relay every bundle)"
                )
            })?;
        vec![selected]
    };

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll_ms = args.poll_ms.unwrap_or(1_000);
    let source_chain_id = source_client.provider.get_chain_id().await?;
    let dest_chain_id = dest_client.provider.get_chain_id().await?;

    for (relay_idx, detected) in to_relay.iter().enumerate() {
        if to_relay.len() > 1 {
            println!(
                "\n--- bundle {}/{} (msg_index={}) ---",
                relay_idx + 1,
                to_relay.len(),
                detected.msg_index
            );
        }

        let (summary, proof) = relay_one(
            &args,
            &source_client,
            &dest_client,
            &dest_rpc.url,
            handler,
            root_storage,
            center,
            tx_hash,
            &receipt,
            detected,
            source_chain_id,
            dest_chain_id,
            timeout,
            poll_ms,
            wallet.as_ref(),
        )
        .await?;

        if args.json {
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }

        if let Some(ref dir) = args.out_dir {
            let suffix = if to_relay.len() > 1 {
                format!("_{}", detected.msg_index)
            } else {
                String::new()
            };
            write_relay_outputs(dir.clone(), suffix, &detected.encoded_bundle, &proof, &summary)
                .await?;
        }
    }

    Ok(())
}

/// Relay a single bundle end-to-end: wait for proof, wait for root, then verify or execute.
#[allow(clippy::too_many_arguments)]
async fn relay_one(
    args: &RelayArgs,
    source_client: &RpcClient,
    dest_client: &RpcClient,
    dest_rpc_url: &str,
    handler: Address,
    root_storage: Address,
    center: Address,
    tx_hash: B256,
    receipt: &TransactionReceipt,
    detected: &DetectedBundle,
    source_chain_id: u64,
    dest_chain_id: u64,
    timeout: Duration,
    poll_ms: u64,
    wallet: Option<&alloy_signer_local::PrivateKeySigner>,
) -> Result<(RelaySummary, MessageInclusionProof)> {
    let log_proof = wait_for_proof(
        source_client,
        receipt.block_number.expect("missing block number"),
        tx_hash,
        detected.msg_index,
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    wait_for_root(
        dest_client,
        root_storage,
        source_chain_id,
        log_proof.batch_number,
        log_proof.root.clone(),
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let proof = build_message_proof(
        &log_proof,
        receipt.transaction_index.expect("missing tx index"),
        center,
        &detected.encoded_bundle,
        source_chain_id,
    );

    let calldata = match args.mode.as_str() {
        "verify" => encode_verify_bundle_call(detected.encoded_bundle.clone(), proof.clone())?,
        "execute" => encode_execute_bundle_call(detected.encoded_bundle.clone(), proof.clone())?,
        other => anyhow::bail!("invalid mode {other} (expected verify or execute)"),
    };

    let mut handler_tx_hash = None;
    if args.dry_run {
        match eth_call(dest_client, handler, calldata.clone()).await {
            Ok(_) => println!("dry-run success"),
            Err(err) => println!("dry-run failed: {err}"),
        }
    } else {
        let wallet = wallet.expect("wallet required");
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .with_chain_id(dest_chain_id)
            .connect(dest_rpc_url)
            .await?;
        let request = alloy_rpc_types::TransactionRequest {
            to: Some(alloy_primitives::TxKind::Call(handler)),
            input: alloy_rpc_types::TransactionInput::new(calldata),
            ..Default::default()
        };
        let pending = decode_send_transaction(provider.send_transaction(request).await)?;
        let htx = pending.tx_hash();
        handler_tx_hash = Some(format!("{htx:#x}"));
        println!("sent tx: {htx:#x}");
    }

    let summary = RelaySummary {
        source_chain_id: source_chain_id.to_string(),
        destination_chain_id: dest_chain_id.to_string(),
        l1_batch_number: proof.l1_batch_number,
        l2_message_index: proof.l2_message_index,
        bundle_hash: format!("{:#x}", detected.bundle_hash),
        source_tx_hash: format!("{tx_hash:#x}"),
        handler_tx_hash,
    };

    Ok((summary, proof))
}

/// Write relay artifacts (bundle, proof, summary) to a directory.
async fn write_relay_outputs(
    dir: PathBuf,
    suffix: String,
    encoded_bundle: &Bytes,
    proof: &MessageInclusionProof,
    summary: &RelaySummary,
) -> Result<()> {
    fs::create_dir_all(&dir)?;
    let bundle_hex = format_hex(&encoded_bundle.0);
    fs::write(dir.join(format!("bundle{suffix}.hex")), &bundle_hex)?;
    fs::write(
        dir.join(format!("proof{suffix}.json")),
        serde_json::to_string_pretty(proof)?,
    )?;
    fs::write(
        dir.join(format!("relay_summary{suffix}.json")),
        serde_json::to_string_pretty(summary)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, Address, Bytes, LogData, B256};
    use alloy_rpc_types::Log;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn log_with_topic(address: Address, topic: B256) -> Log {
        Log {
            inner: alloy_primitives::Log {
                address,
                data: LogData::new(vec![topic], Bytes::new()).unwrap(),
            },
            block_hash: None,
            block_number: None,
            block_timestamp: None,
            transaction_hash: None,
            transaction_index: None,
            log_index: None,
            removed: false,
        }
    }

    fn l1_msg_log() -> Log {
        log_with_topic(L1_SENDER_ADDRESS, l1_message_sent_topic())
    }

    fn bundle_log(center: Address) -> Log {
        log_with_topic(center, interop_bundle_sent_topic())
    }

    const CENTER: Address = address!("0000000000000000000000000000000000010010");
    const OTHER: Address = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

    // ── Tests for find_bundle_log_positions ──────────────────────────────────

    #[test]
    fn no_logs_returns_empty() {
        assert!(find_bundle_log_positions(&[], CENTER).is_empty());
    }

    #[test]
    fn no_bundle_logs_returns_empty() {
        let logs = vec![l1_msg_log(), l1_msg_log()];
        assert!(find_bundle_log_positions(&logs, CENTER).is_empty());
    }

    #[test]
    fn single_bundle_after_one_l1_msg_gets_index_zero() {
        // Pattern: [L1Msg, Bundle]  → bundle's own L1Msg is the first counted, so msg_index = 0
        let logs = vec![l1_msg_log(), bundle_log(CENTER)];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0], (1, 0)); // (log_idx=1, msg_index=0)
    }

    #[test]
    fn two_bundles_get_sequential_msg_indices() {
        // [L1Msg, Bundle_A, L1Msg, Bundle_B]
        let logs = vec![
            l1_msg_log(),       // consumed by A → A.msg_index = 0
            bundle_log(CENTER),
            l1_msg_log(),       // consumed by B → B.msg_index = 1
            bundle_log(CENTER),
        ];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0], (1, 0));
        assert_eq!(positions[1], (3, 1));
    }

    #[test]
    fn unrelated_l1_msgs_before_first_bundle_shift_index() {
        // An extra L1MessageSent before any bundle (e.g., from some other contract action)
        // increases the count, so the first bundle gets msg_index = 1, not 0.
        let logs = vec![
            l1_msg_log(),       // unrelated msg → count=1
            l1_msg_log(),       // bundle A's msg → count=2, A.msg_index = 1
            bundle_log(CENTER),
            l1_msg_log(),       // bundle B's msg → count=3, B.msg_index = 2
            bundle_log(CENTER),
        ];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].1, 1); // msg_index of first bundle
        assert_eq!(positions[1].1, 2); // msg_index of second bundle
    }

    #[test]
    fn l1_msgs_from_wrong_address_not_counted() {
        let logs = vec![
            // Same topic, but wrong address — must NOT be counted.
            log_with_topic(OTHER, l1_message_sent_topic()),
            l1_msg_log(),       // only this one counts → count=1, msg_index=0
            bundle_log(CENTER),
        ];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].1, 0);
    }

    #[test]
    fn bundle_from_wrong_center_not_returned() {
        let logs = vec![
            l1_msg_log(),
            bundle_log(OTHER), // different center — must be ignored
        ];
        let positions = find_bundle_log_positions(&logs, CENTER);
        assert!(positions.is_empty());
    }

    #[test]
    fn bundle_before_any_l1_msg_gets_max_saturating_zero() {
        // Bundle appears before its L1MessageSent: saturating_sub(1) on 0 → 0.
        // This is an edge case that should not happen on a real chain, but the
        // function must not panic.
        let logs = vec![bundle_log(CENTER)];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].1, 0);
    }

    #[test]
    fn mixed_centers_only_target_returned() {
        // Logs from two different centers; we only want `CENTER`.
        let logs = vec![
            l1_msg_log(),
            bundle_log(OTHER),  // ignored
            l1_msg_log(),
            bundle_log(CENTER), // counted: msg_index = 1
        ];
        let positions = find_bundle_log_positions(&logs, CENTER);

        assert_eq!(positions.len(), 1);
        // Two L1Msgs counted before the CENTER bundle → msg_index = 1
        assert_eq!(positions[0].1, 1);
    }
}
