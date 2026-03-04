use crate::cli::BalancesArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{parse_address, AddressBook};
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::Provider;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;

// ────────────────────────────────────────────────────────────────────────────
// Output types
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBalance {
    pub token: String,
    pub symbol: String,
    pub decimals: u8,
    pub raw: String,
    pub formatted: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainBalances {
    pub chain: String,
    pub rpc: String,
    pub chain_id: u64,
    pub native_wei: String,
    pub native_formatted: String,
    pub tokens: Vec<TokenBalance>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalancesOutput {
    pub address: String,
    pub chains: Vec<ChainBalances>,
}

// ────────────────────────────────────────────────────────────────────────────
// ERC-20 minimal ABI helpers  (balanceOf / decimals / symbol)
// ────────────────────────────────────────────────────────────────────────────

/// `balanceOf(address)` → `uint256`
fn encode_balance_of(addr: Address) -> Bytes {
    let mut data = vec![0x70u8, 0xa0, 0x82, 0x31]; // selector
    let mut padded = [0u8; 32];
    padded[12..].copy_from_slice(addr.as_slice());
    data.extend_from_slice(&padded);
    Bytes::from(data)
}

/// `decimals()` → `uint8`
fn encode_decimals() -> Bytes {
    Bytes::from(vec![0x31u8, 0x3c, 0xe5, 0x67])
}

/// `symbol()` → `string`
fn encode_symbol() -> Bytes {
    Bytes::from(vec![0x95u8, 0xd8, 0x9b, 0x41])
}

async fn fetch_uint256(client: &RpcClient, token: Address, calldata: Bytes) -> Result<U256> {
    let raw = eth_call(client, token, calldata).await?;
    if raw.len() < 32 {
        anyhow::bail!("short response");
    }
    Ok(U256::from_be_slice(&raw[..32]))
}

async fn fetch_decimals(client: &RpcClient, token: Address) -> u8 {
    fetch_uint256(client, token, encode_decimals())
        .await
        .map(|v| v.to::<u8>())
        .unwrap_or(18)
}

/// Decode an ABI-encoded `string` return value.
fn decode_abi_string(raw: &[u8]) -> String {
    // layout: offset(32) | length(32) | utf8 bytes
    if raw.len() < 64 {
        return "???".to_string();
    }
    let len = U256::from_be_slice(&raw[32..64]).to::<usize>();
    if raw.len() < 64 + len {
        return "???".to_string();
    }
    String::from_utf8_lossy(&raw[64..64 + len]).to_string()
}

async fn fetch_symbol(client: &RpcClient, token: Address) -> String {
    eth_call(client, token, encode_symbol())
        .await
        .map(|raw| decode_abi_string(&raw))
        .unwrap_or_else(|_| "???".to_string())
}

// ────────────────────────────────────────────────────────────────────────────
// Formatting
// ────────────────────────────────────────────────────────────────────────────

fn format_units(raw: U256, decimals: u8) -> String {
    if decimals == 0 {
        return raw.to_string();
    }
    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = raw / divisor;
    let frac = raw % divisor;
    if frac.is_zero() {
        return whole.to_string();
    }
    let frac_str = format!("{:0>width$}", frac, width = decimals as usize);
    let trimmed = frac_str.trim_end_matches('0');
    format!("{}.{}", whole, trimmed)
}

// ────────────────────────────────────────────────────────────────────────────
// Per-chain query (runs in a spawned task)
// ────────────────────────────────────────────────────────────────────────────

async fn query_chain(
    alias: String,
    rpc_url: String,
    wallet: Address,
    token_addresses: Vec<Address>,
) -> ChainBalances {
    let client = match RpcClient::new(&rpc_url).await {
        Ok(c) => c,
        Err(err) => {
            return ChainBalances {
                chain: alias,
                rpc: rpc_url,
                chain_id: 0,
                native_wei: "0".to_string(),
                native_formatted: "0".to_string(),
                tokens: vec![],
                error: Some(format!("rpc connect failed: {err}")),
            };
        }
    };

    let chain_id = client.provider.get_chain_id().await.unwrap_or(0);

    // native balance
    let native = client.provider.get_balance(wallet).await.unwrap_or(U256::ZERO);

    // ERC-20 balances (sequential within a chain — usually only a few tokens)
    let mut tokens = Vec::with_capacity(token_addresses.len());
    for token_addr in &token_addresses {
        let symbol = fetch_symbol(&client, *token_addr).await;
        let decimals = fetch_decimals(&client, *token_addr).await;
        let balance = fetch_uint256(&client, *token_addr, encode_balance_of(wallet))
            .await
            .unwrap_or(U256::ZERO);
        tokens.push(TokenBalance {
            token: format!("{token_addr:#x}"),
            symbol,
            decimals,
            raw: balance.to_string(),
            formatted: format_units(balance, decimals),
        });
    }

    ChainBalances {
        chain: alias,
        rpc: rpc_url,
        chain_id,
        native_wei: native.to_string(),
        native_formatted: format_units(native, 18),
        tokens,
        error: None,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Command entry-point
// ────────────────────────────────────────────────────────────────────────────

pub async fn run(args: BalancesArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let wallet = parse_address(&args.address).context("invalid --address")?;

    // Parse optional token list
    let token_addresses: Vec<Address> = args
        .token
        .iter()
        .map(|t| parse_address(t).context("invalid token address"))
        .collect::<Result<Vec<_>>>()?;

    // Collect chains to query
    let chains: BTreeMap<String, String> = match (args.chain.as_deref(), &config.chains) {
        // Explicit single chain
        (Some(alias), _) => {
            let resolved = config
                .resolve_rpc(None, Some(alias))
                .with_context(|| format!("unknown chain alias: {alias}"))?;
            [(alias.to_string(), resolved.url)].into()
        }
        // All configured chains
        (None, Some(chain_map)) if !chain_map.is_empty() => chain_map
            .iter()
            .map(|(k, v)| (k.clone(), v.rpc.clone()))
            .collect(),
        // Explicit RPC URL
        _ => {
            if let Some(rpc) = &args.rpc {
                [("rpc".to_string(), rpc.clone())].into()
            } else {
                anyhow::bail!(
                    "no chains configured and no --rpc/--chain provided. \
                     Add chains with `cast-interop chains add` or pass --rpc."
                );
            }
        }
    };

    // Spawn one task per chain (true parallel I/O)
    let mut handles = Vec::with_capacity(chains.len());
    for (alias, rpc_url) in chains {
        let wallet_clone = wallet;
        let tokens_clone = token_addresses.clone();
        handles.push(tokio::spawn(query_chain(
            alias,
            rpc_url,
            wallet_clone,
            tokens_clone,
        )));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.context("chain query task panicked")?);
    }

    // Sort by chain name for deterministic output
    results.sort_by(|a, b| a.chain.cmp(&b.chain));

    let output = BalancesOutput {
        address: format!("{wallet:#x}"),
        chains: results,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Human-readable output
    println!("Balances for {:#x}", wallet);
    for chain in &output.chains {
        println!("\n  chain: {} (id={})", chain.chain, chain.chain_id);
        if let Some(err) = &chain.error {
            println!("    ⚠ error: {err}");
            continue;
        }
        println!("    native: {} ETH  ({}  wei)", chain.native_formatted, chain.native_wei);
        for tok in &chain.tokens {
            println!(
                "    {}: {}  ({} wei)  [{}]",
                tok.symbol, tok.formatted, tok.raw, tok.token
            );
        }
        if chain.tokens.is_empty() && args.token.is_empty() {
            println!("    (pass --token 0xADDR to include ERC-20 balances)");
        }
    }

    Ok(())
}
