use crate::cli::ProofArgs;
use crate::config::Config;
use crate::rpc::{
    get_transaction_receipt, wait_for_finalized_block, wait_for_log_proof, RpcClient,
};
use crate::types::{AddressBook, MessageInclusionProof, ProofMessage};
use alloy_primitives::B256;
use alloy_provider::Provider;
use anyhow::{Context, Result};
use std::fs;
use std::str::FromStr;
use std::time::Duration;

/// Fetch the L2→L1 log proof for an interop transaction.
///
/// Waits for finalization (unless disabled) and writes the proof as JSON.
pub async fn run(args: ProofArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&client, tx_hash).await?;

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll_ms = args.poll_ms.unwrap_or(1_000);

    if !args.no_wait {
        wait_for_finalized_block(
            &client,
            receipt.block_number.expect("missing block number"),
            timeout,
            Duration::from_millis(100),
        )
        .await?;
    }

    let log_proof = wait_for_log_proof(
        &client,
        tx_hash,
        args.msg_index,
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let chain_id = client.provider.get_chain_id().await?.to_string();
    let message = ProofMessage {
        tx_number_in_batch: receipt.transaction_index.expect("missing tx index"),
        sender: format!("{:#x}", addresses.interop_center),
        data: "0x".to_string(),
    };
    let output = MessageInclusionProof {
        chain_id,
        l1_batch_number: log_proof.batch_number,
        l2_message_index: log_proof.id,
        root: log_proof.root.clone(),
        message,
        proof: log_proof.proof.clone(),
    };

    println!("Message inclusion proof obtained:");

    if args.json || args.out.is_some() {
        let json = serde_json::to_string_pretty(&output)?;
        if args.json {
            println!("{json}");
        }
        if let Some(path) = args.out {
            fs::write(path, json)?;
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}
