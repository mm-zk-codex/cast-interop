use crate::abi::{decode_bytes32, encode_execute_bundle_call, encode_interop_roots_call};
use crate::commands::bundle_action::decode_send_transaction;
use crate::rpc::{eth_call, wait_for_finalized_block, wait_for_log_proof, LogProof, RpcClient};
use crate::types::{MessageInclusionProof, ProofMessage, BUNDLE_IDENTIFIER};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use anyhow::Result;
use std::time::Duration;

pub async fn wait_for_root(
    client: &RpcClient,
    root_storage: Address,
    chain_id: u64,
    batch_number: u64,
    expected_root: String,
    timeout: Duration,
    poll: Duration,
) -> Result<()> {
    let expected = B256::from_str(&expected_root)?;
    let start = tokio::time::Instant::now();
    let mut first_run = true;
    loop {
        let data = encode_interop_roots_call(U256::from(chain_id), U256::from(batch_number));
        let result = eth_call(client, root_storage, data).await?;
        let root = decode_bytes32(result)?;
        if root != B256::ZERO {
            if root == expected {
                println!("interop root available: {root:#x}");
                return Ok(());
            }
            anyhow::bail!("interop root mismatch: expected {expected:#x}, got {root:#x}");
        }
        if start.elapsed() > timeout {
            anyhow::bail!("interop root did not become available in time");
        }
        if first_run {
            println!("waiting for interop root to become available for {timeout:?}...");
            first_run = false;
        }
        tokio::time::sleep(poll).await;
    }
}

pub async fn wait_for_proof(
    client: &RpcClient,
    block_number: u64,
    tx_hash: B256,
    msg_index: u32,
    timeout: Duration,
    poll: Duration,
) -> Result<LogProof> {
    wait_for_finalized_block(client, block_number, timeout, Duration::from_millis(100)).await?;
    wait_for_log_proof(client, tx_hash, msg_index, timeout, poll).await
}

pub fn build_message_proof(
    log_proof: &LogProof,
    tx_index: u64,
    center: Address,
    encoded_bundle: &Bytes,
    source_chain_id: u64,
) -> MessageInclusionProof {
    let message = ProofMessage {
        tx_number_in_batch: tx_index,
        sender: format!("{center:#x}"),
        data: format!(
            "0x{}{}",
            hex::encode([BUNDLE_IDENTIFIER]),
            hex::encode(encoded_bundle.as_ref())
        ),
    };
    MessageInclusionProof {
        chain_id: source_chain_id.to_string(),
        l1_batch_number: log_proof.batch_number,
        l2_message_index: log_proof.id,
        root: log_proof.root.clone(),
        message,
        proof: log_proof.proof.clone(),
    }
}

pub async fn execute_bundle(
    client: &RpcClient,
    rpc_url: &str,
    handler: Address,
    signer: alloy_signer_local::PrivateKeySigner,
    encoded_bundle: Bytes,
    proof: MessageInclusionProof,
) -> Result<B256> {
    let calldata = encode_execute_bundle_call(encoded_bundle, proof)?;
    let chain_id = client.provider.get_chain_id().await?;

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .with_chain_id(chain_id)
        .connect(rpc_url)
        .await?;

    let request = alloy_rpc_types::TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(handler)),
        input: alloy_rpc_types::TransactionInput::new(calldata),
        ..Default::default()
    };

    let pending = decode_send_transaction(provider.send_transaction(request).await)?;
    Ok(*pending.tx_hash())
}

use std::str::FromStr;
