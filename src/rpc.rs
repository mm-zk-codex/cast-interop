use alloy_primitives::{Address, Bytes, B256};
use alloy_provider::{DynProvider, Provider, ProviderBuilder};
use alloy_rpc_types::{BlockNumberOrTag, TransactionInput, TransactionReceipt, TransactionRequest};
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

use crate::commands::bundle_action::decode_revert_reason;

#[derive(Clone, Debug)]
pub struct RpcClient {
    pub url: String,
    pub provider: DynProvider,
    pub http: Client,
}

impl RpcClient {
    pub async fn new(url: &str) -> Result<Self> {
        let http = Client::new();

        let provider = ProviderBuilder::new().connect(url).await?;

        Ok(Self {
            url: url.to_string(),
            provider: provider.erased(),
            http,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogProof {
    pub id: u64,
    pub proof: Vec<String>,
    pub root: String,
    #[serde(rename = "batch_number")]
    pub batch_number: u64,
}

pub async fn get_transaction_receipt(
    client: &RpcClient,
    tx_hash: B256,
) -> Result<TransactionReceipt> {
    client
        .provider
        .get_transaction_receipt(tx_hash)
        .await?
        .ok_or_else(|| anyhow!("transaction receipt not found"))
}

pub async fn get_finalized_block_number(client: &RpcClient) -> Result<u64> {
    let block = client
        .provider
        .get_block_by_number(BlockNumberOrTag::Finalized)
        .await?
        .ok_or_else(|| anyhow!("finalized block not found"))?;
    Ok(block.header.number)
}

pub async fn wait_for_finalized_block(
    client: &RpcClient,
    block_number: u64,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<()> {
    let start = tokio::time::Instant::now();
    loop {
        let finalized = get_finalized_block_number(client).await.unwrap_or(0);
        if finalized >= block_number {
            return Ok(());
        }
        if start.elapsed() > timeout {
            anyhow::bail!("block was not finalized in time");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn get_log_proof(
    client: &RpcClient,
    tx_hash: B256,
    msg_index: u32,
) -> Result<Option<LogProof>> {
    let hash_hex = format!("{tx_hash:#x}");
    let params = json!([hash_hex, msg_index]);
    raw_rpc::<Option<LogProof>>(client, "zks_getL2ToL1LogProof", params.clone()).await
}

pub async fn wait_for_log_proof(
    client: &RpcClient,
    tx_hash: B256,
    msg_index: u32,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<LogProof> {
    let start = tokio::time::Instant::now();
    loop {
        if let Some(proof) = get_log_proof(client, tx_hash, msg_index).await? {
            return Ok(proof);
        }
        if start.elapsed() > timeout {
            anyhow::bail!("log proof not available in time");
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub async fn raw_rpc<T: for<'de> Deserialize<'de>>(
    client: &RpcClient,
    method: &str,
    params: serde_json::Value,
) -> Result<T> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });
    let response = client
        .http
        .post(&client.url)
        .json(&payload)
        .send()
        .await
        .context("rpc request failed")?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.context("rpc decode failed")?;
    if !status.is_success() {
        anyhow::bail!("rpc error status {status}: {value}");
    }
    if let Some(error) = value.get("error") {
        anyhow::bail!("rpc error: {error}");
    }
    serde_json::from_value(value.get("result").cloned().unwrap_or_default())
        .context("rpc missing result")
}

pub async fn eth_call(client: &RpcClient, to: Address, data: Bytes) -> Result<Bytes> {
    eth_call_with_value(client, to, data, None).await
}

pub async fn eth_call_with_value(
    client: &RpcClient,
    to: Address,
    data: Bytes,
    value: Option<alloy_primitives::U256>,
) -> Result<Bytes> {
    let request = TransactionRequest {
        to: Some(to.into()),
        input: TransactionInput::new(data),
        value,
        ..Default::default()
    };
    let result = client.provider.call(request).await;

    let result = match result {
        Ok(result) => result,
        Err(err) => {
            if let Some(reason) = decode_revert_reason(err.to_string()) {
                return Err(anyhow!("dry-run reverted: {reason}"));
            } else {
                return Err(anyhow!("dry-run failed: {err}"));
            }
        }
    };
    Ok(result)
}

pub async fn estimate_gas(
    client: &RpcClient,
    to: Address,
    data: Bytes,
    value: Option<alloy_primitives::U256>,
    from: Option<Address>,
) -> Result<u64> {
    let request = TransactionRequest {
        from,
        to: Some(to.into()),
        input: TransactionInput::new(data),
        value,
        ..Default::default()
    };
    Ok(client.provider.estimate_gas(request).await?)
}

pub async fn get_gas_price(client: &RpcClient) -> Result<u128> {
    Ok(client.provider.get_gas_price().await?)
}

pub fn format_gas_estimate(gas: u64, gas_price: u128) -> String {
    let cost_wei = gas as u128 * gas_price;
    let gas_price_gwei = gas_price as f64 / 1e9;
    let cost_eth = cost_wei as f64 / 1e18;
    format!("estimated gas: {gas}  |  gas price: {gas_price_gwei:.3} gwei  |  cost: {cost_eth:.9} ETH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_gas_estimate_typical_values() {
        // 100,000 gas at 1 gwei = 0.0001 ETH
        let result = format_gas_estimate(100_000, 1_000_000_000);
        assert!(result.contains("100000"), "missing gas units");
        assert!(result.contains("1.000 gwei"), "missing gas price");
        assert!(result.contains("0.000100000 ETH"), "missing cost");
    }

    #[test]
    fn format_gas_estimate_zero_gas() {
        let result = format_gas_estimate(0, 1_000_000_000);
        assert!(result.contains("0.000000000 ETH"));
    }

    #[test]
    fn format_gas_estimate_high_gas_price() {
        // 21,000 gas at 100 gwei = 0.0021 ETH
        let result = format_gas_estimate(21_000, 100_000_000_000);
        assert!(result.contains("21000"));
        assert!(result.contains("100.000 gwei"));
        assert!(result.contains("0.002100000 ETH"));
    }

    #[test]
    fn format_gas_estimate_includes_all_separators() {
        let result = format_gas_estimate(1, 1);
        assert!(result.contains("estimated gas:"));
        assert!(result.contains("gas price:"));
        assert!(result.contains("cost:"));
    }
}
