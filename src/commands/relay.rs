use crate::abi::{
    decode_interop_bundle_sent, encode_execute_bundle_call, encode_interop_bundle,
    encode_interop_roots_call, encode_verify_bundle_call, interop_bundle_sent_topic,
};
use crate::cli::RelayArgs;
use crate::config::Config;
use crate::rpc::{
    eth_call, get_transaction_receipt, wait_for_finalized_block, wait_for_log_proof, RpcClient,
};
use crate::types::{
    format_hex, require_signer_or_dry_run, AddressBook, MessageInclusionProof, ProofMessage,
    RelaySummary, BUNDLE_IDENTIFIER,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_signer_local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub async fn run(args: RelayArgs, config: Config, addresses: AddressBook) -> Result<()> {
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
    let root_storage = args
        .root_storage
        .as_deref()
        .map(|value| Address::from_str(value))
        .transpose()
        .context("invalid root storage address")?
        .unwrap_or(addresses.interop_root_storage);

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

    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "relay")?;

    let source_client = RpcClient::new(&args.rpc_src).await?;
    let dest_client = RpcClient::new(&args.rpc_dest).await?;

    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&source_client, tx_hash).await?;
    let mut bundle = None;
    let mut bundle_hash = None;
    for log in receipt.logs().iter() {
        if log.topics().first().copied() == Some(interop_bundle_sent_topic()) {
            let (_, hash, interop_bundle) = decode_interop_bundle_sent(log.data().data.clone())?;
            bundle = Some(interop_bundle);
            bundle_hash = Some(hash);
            break;
        }
    }
    let bundle = bundle.ok_or_else(|| anyhow!("InteropBundleSent not found in receipt"))?;
    let bundle_hash = bundle_hash.expect("bundle hash");
    let encoded_bundle = encode_interop_bundle(&bundle);

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll_ms = args.poll_ms.unwrap_or(1_000);

    wait_for_finalized_block(
        &source_client,
        receipt.block_number.expect("missing block number"),
        timeout,
        Duration::from_millis(100),
    )
    .await?;
    let log_proof = wait_for_log_proof(
        &source_client,
        tx_hash,
        args.msg_index,
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let source_chain_id = source_client.provider.get_chain_id().await?;
    let expected_root = log_proof.root.clone();

    wait_for_root(
        &dest_client,
        root_storage,
        source_chain_id,
        log_proof.batch_number,
        expected_root.clone(),
        timeout,
        Duration::from_millis(poll_ms),
    )
    .await?;

    let message = ProofMessage {
        tx_number_in_batch: receipt.transaction_index.expect("missing tx index"),
        sender: format!("{center:#x}"),
        data: format!(
            "0x{}{}",
            hex::encode([BUNDLE_IDENTIFIER]),
            hex::encode(encoded_bundle.as_ref())
        ),
    };
    let proof = MessageInclusionProof {
        chain_id: source_chain_id.to_string(),
        l1_batch_number: log_proof.batch_number,
        l2_message_index: log_proof.id,
        root: log_proof.root.clone(),
        message,
        proof: log_proof.proof.clone(),
    };

    let calldata = match args.mode.as_str() {
        "verify" => encode_verify_bundle_call(encoded_bundle.clone(), proof.clone())?,
        "execute" => encode_execute_bundle_call(encoded_bundle.clone(), proof.clone())?,
        other => anyhow::bail!("invalid mode {other} (expected verify or execute)"),
    };

    let mut handler_tx_hash = None;
    if args.dry_run {
        match eth_call(&dest_client, handler, calldata.clone()).await {
            Ok(_) => println!("dry-run success"),
            Err(err) => println!("dry-run failed: {err}"),
        }
    } else {
        let wallet = wallet.expect("wallet required");
        let chain_id = dest_client.provider.get_chain_id().await?;

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .with_chain_id(chain_id)
            .connect(&args.rpc_dest)
            .await?;
        let request = alloy_rpc_types::TransactionRequest {
            to: Some(alloy_primitives::TxKind::Call(handler)),
            input: alloy_rpc_types::TransactionInput::new(calldata),
            ..Default::default()
        };
        let pending = provider.send_transaction(request).await?;
        let tx_hash = pending.tx_hash();
        handler_tx_hash = Some(format!("{tx_hash:#x}"));
        println!("sent tx: {tx_hash:#x}");
    }

    if let Some(dir) = args.out_dir {
        write_relay_outputs(
            dir,
            &encoded_bundle,
            &proof,
            &bundle_hash,
            &tx_hash,
            handler_tx_hash.clone(),
            &source_chain_id,
            &dest_client,
        )
        .await?;
    }

    Ok(())
}

async fn wait_for_root(
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
    loop {
        let data = encode_interop_roots_call(U256::from(chain_id), U256::from(batch_number));
        let result = eth_call(client, root_storage, data).await?;
        let root = crate::abi::decode_bytes32(result)?;
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
        tokio::time::sleep(poll).await;
    }
}

async fn write_relay_outputs(
    dir: PathBuf,
    encoded_bundle: &Bytes,
    proof: &MessageInclusionProof,
    bundle_hash: &B256,
    source_tx_hash: &B256,
    handler_tx_hash: Option<String>,
    source_chain_id: &u64,
    dest_client: &RpcClient,
) -> Result<()> {
    fs::create_dir_all(&dir)?;
    let bundle_hex = format_hex(&encoded_bundle.0);
    fs::write(dir.join("bundle.hex"), &bundle_hex)?;
    fs::write(dir.join("proof.json"), serde_json::to_string_pretty(proof)?)?;

    let dest_chain_id = dest_client.provider.get_chain_id().await?;
    let summary = RelaySummary {
        source_chain_id: source_chain_id.to_string(),
        destination_chain_id: dest_chain_id.to_string(),
        l1_batch_number: proof.l1_batch_number,
        l2_message_index: proof.l2_message_index,
        bundle_hash: format!("{bundle_hash:#x}"),
        source_tx_hash: format!("{source_tx_hash:#x}"),
        handler_tx_hash,
    };
    fs::write(
        dir.join("relay_summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    Ok(())
}

fn load_wallet(key: &str) -> Result<PrivateKeySigner> {
    let pk_signer: PrivateKeySigner = key.parse()?;
    Ok(pk_signer)
}
