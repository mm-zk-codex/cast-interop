use crate::cli::RpcPingArgs;
use crate::config::Config;
use crate::rpc::{get_finalized_block_number, raw_rpc, RpcClient};
use crate::types::AddressBook;
use alloy_provider::Provider;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RpcPingOutput {
    chain_id: Option<String>,
    latest_block: Option<u64>,
    finalized_block: Option<String>,
    client_version: Option<String>,
    chain_type: String,
    zks_methods_available: Option<bool>,
}

/// Check RPC connectivity and feature support.
///
/// Reports chain ID, latest/finalized blocks, client version, and zkSync method availability.
/// For L1 chains (configured with --l1), zkSync-specific methods are not checked.
pub async fn run(args: RpcPingArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;

    let chain_id = client
        .provider
        .get_chain_id()
        .await
        .ok()
        .map(|id| id.to_string());
    let latest_block = client.provider.get_block_number().await.ok();
    let finalized_block = match get_finalized_block_number(&client).await {
        Ok(value) => Some(value.to_string()),
        Err(_) => None,
    };
    let client_version = raw_rpc::<String>(&client, "web3_clientVersion", json!([]))
        .await
        .ok();

    // For L1 chains, zks_* methods are not expected.
    let zks_methods_available = if resolved.is_l1 {
        None
    } else {
        let probe: Result<Option<serde_json::Value>> =
            raw_rpc(&client, "zks_L1ChainId", json!([])).await;
        Some(probe.is_ok())
    };

    let chain_type = if resolved.is_l1 {
        "L1 (Ethereum)".to_string()
    } else {
        "L2 (zkSync)".to_string()
    };

    let output = RpcPingOutput {
        chain_id,
        latest_block,
        finalized_block,
        client_version,
        chain_type,
        zks_methods_available,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!(
        "chainId: {}",
        output
            .chain_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!("chainType: {}", output.chain_type);
    println!(
        "latest block: {}",
        output
            .latest_block
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "finalized block: {}",
        output
            .finalized_block
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!(
        "client version: {}",
        output
            .client_version
            .clone()
            .unwrap_or_else(|| "n/a".to_string())
    );
    match output.zks_methods_available {
        Some(true) => println!("zks_* methods: available"),
        Some(false) => println!("zks_* methods: not available"),
        None => println!("zks_* methods: not checked (L1 chain)"),
    }

    Ok(())
}
