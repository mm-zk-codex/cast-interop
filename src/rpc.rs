use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_types::{BlockNumberOrTag, TransactionReceipt, TransactionRequest};
use alloy_transport_http::Http;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Clone)]
pub struct RpcClient {
    pub url: String,
    pub provider: RootProvider<Http<Client>>,
    pub http: Client,
}

impl RpcClient {
    pub fn new(url: &str) -> Result<Self> {
        let http = Client::new();
        let transport = Http::new(url.parse()?);
        let provider = ProviderBuilder::new().on_http(transport);
        Ok(Self {
            url: url.to_string(),
            provider,
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
        .get_block_by_number(BlockNumberOrTag::Finalized, false)
        .await?
        .ok_or_else(|| anyhow!("finalized block not found"))?;
    Ok(block.header.number.unwrap_or_default())
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
    if let Ok(result) = raw_rpc::<Option<LogProof>>(client, "zks_getLogProof", params.clone()).await
    {
        return Ok(result);
    }
    raw_rpc::<Option<LogProof>>(client, "getLogProof", params).await
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
    let request = TransactionRequest {
        to: Some(to),
        data: Some(data),
        ..Default::default()
    };
    let result = client.provider.call(&request, None).await?;
    Ok(result)
}

pub async fn estimate_gas(
    client: &RpcClient,
    from: Address,
    to: Address,
    data: Bytes,
) -> Result<U256> {
    let request = TransactionRequest {
        from: Some(from),
        to: Some(to),
        data: Some(data),
        ..Default::default()
    };
    Ok(client.provider.estimate_gas(&request, None).await?)
}

pub async fn send_raw_transaction(client: &RpcClient, raw_tx: Bytes) -> Result<B256> {
    Ok(client.provider.send_raw_transaction(raw_tx).await?)
}
