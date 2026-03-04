use crate::cli::BalancesArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{parse_address, AddressBook};
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::Provider;
use anyhow::{Context, Result};
use serde::Serialize;

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

/// Encode `balanceOf(address)` → `uint256` calldata.
fn encode_balance_of(addr: Address) -> Bytes {
    let mut data = vec![0x70u8, 0xa0, 0x82, 0x31]; // keccak256("balanceOf(address)")[..4]
    let mut padded = [0u8; 32];
    padded[12..].copy_from_slice(addr.as_slice());
    data.extend_from_slice(&padded);
    Bytes::from(data)
}

/// Encode `decimals()` → `uint8` calldata.
fn encode_decimals() -> Bytes {
    Bytes::from(vec![0x31u8, 0x3c, 0xe5, 0x67]) // keccak256("decimals()")[..4]
}

/// Encode `symbol()` → `string` calldata.
fn encode_symbol() -> Bytes {
    Bytes::from(vec![0x95u8, 0xd8, 0x9b, 0x41]) // keccak256("symbol()")[..4]
}

async fn fetch_uint256(client: &RpcClient, token: Address, calldata: Bytes) -> Result<U256> {
    let raw = eth_call(client, token, calldata).await?;
    if raw.len() < 32 {
        anyhow::bail!("response too short ({} bytes)", raw.len());
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
///
/// Expected layout: `offset (32 bytes) | length (32 bytes) | utf8 data`.
/// Falls back to interpreting the first 32 bytes as a right-padded `bytes32`
/// (used by some tokens such as MKR that return a fixed-size type instead of
/// the dynamic string type).
fn decode_abi_string(raw: &[u8]) -> String {
    // At least offset + length words needed for a proper ABI string.
    if raw.len() >= 64 {
        let len = U256::from_be_slice(&raw[32..64]).to::<usize>();
        // Guard against absurdly large lengths (e.g. a bytes32 misread as len).
        if len <= 256 && raw.len() >= 64 + len {
            let s = String::from_utf8_lossy(&raw[64..64 + len]).to_string();
            // Only accept if it looks like a readable symbol.
            if s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) && !s.is_empty() {
                return s;
            }
        }
    }
    // Fallback: bytes32 — strip trailing NUL bytes and interpret as UTF-8.
    if raw.len() >= 32 {
        let end = raw[..32]
            .iter()
            .rposition(|&b| b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        if end > 0 {
            if let Ok(s) = std::str::from_utf8(&raw[..end]) {
                if !s.is_empty() && s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
                    return s.to_string();
                }
            }
        }
    }
    "???".to_string()
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

/// Format a raw `U256` amount with `decimals` fixed-point places.
///
/// Examples:
/// * `format_units(U256::from(1_500_000_000_000_000_000u64), 18)` → `"1.5"`
/// * `format_units(U256::ZERO, 18)` → `"0"`
/// * `format_units(U256::from(1u64), 0)` → `"1"`
pub fn format_units(raw: U256, decimals: u8) -> String {
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
    format!("{whole}.{trimmed}")
}

// ────────────────────────────────────────────────────────────────────────────
// Per-chain query (runs in a spawned task)
// ────────────────────────────────────────────────────────────────────────────

async fn query_chain(
    alias: String,
    rpc_url: String,
    config_chain_id: Option<u64>,
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

    // Use the chain_id from config when available to save a round-trip.
    let chain_id = match config_chain_id {
        Some(id) => id,
        None => client.provider.get_chain_id().await.unwrap_or(0),
    };

    // Native balance.
    let native = client.provider.get_balance(wallet).await.unwrap_or(U256::ZERO);

    // For each token: fetch symbol, decimals, and balance concurrently.
    // Each future gets its own clone of the client (cheap — both DynProvider
    // and reqwest::Client are Arc-backed internally).
    let tokens: Vec<TokenBalance> = futures::future::join_all(
        token_addresses.iter().map(|&token_addr| {
            let c = client.clone();
            async move {
                let (symbol, decimals, balance) = tokio::join!(
                    fetch_symbol(&c, token_addr),
                    fetch_decimals(&c, token_addr),
                    fetch_uint256(&c, token_addr, encode_balance_of(wallet)),
                );
                let balance = balance.unwrap_or(U256::ZERO);
                TokenBalance {
                    token: format!("{token_addr:#x}"),
                    symbol,
                    decimals,
                    raw: balance.to_string(),
                    formatted: format_units(balance, decimals),
                }
            }
        }),
    )
    .await;

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
    // Validate mutual exclusion early for a clear error message.
    if args.rpc.is_some() && args.chain.is_some() {
        anyhow::bail!("--rpc and --chain are mutually exclusive; use one or the other");
    }

    let wallet = parse_address(&args.address).context("invalid --address")?;

    // Parse optional token list.
    let token_addresses: Vec<Address> = args
        .token
        .iter()
        .map(|t| parse_address(t).context("invalid token address"))
        .collect::<Result<Vec<_>>>()?;

    // Collect (alias, rpc_url, optional_chain_id) tuples.
    // Priority: --chain > --rpc > all configured chains.
    struct ChainTarget {
        alias: String,
        rpc: String,
        chain_id: Option<u64>,
    }

    let targets: Vec<ChainTarget> = if let Some(alias) = args.chain.as_deref() {
        // Single named chain from config.
        let resolved = config
            .resolve_rpc(None, Some(alias))
            .with_context(|| format!("unknown chain alias: {alias}"))?;
        let chain_id = config
            .chain(alias)
            .and_then(|c| c.chain_id);
        vec![ChainTarget { alias: alias.to_string(), rpc: resolved.url, chain_id }]
    } else if let Some(rpc_url) = &args.rpc {
        // Bare RPC URL — use hostname as the display label.
        let alias = url::Url::parse(rpc_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "rpc".to_string());
        vec![ChainTarget { alias, rpc: rpc_url.clone(), chain_id: None }]
    } else if let Some(chain_map) = &config.chains {
        if chain_map.is_empty() {
            anyhow::bail!(
                "no chains configured. Add one with `cast-interop chains add <alias> --rpc <url>` \
                 or pass --rpc / --chain."
            );
        }
        chain_map
            .iter()
            .map(|(k, v)| ChainTarget {
                alias: k.clone(),
                rpc: v.rpc.clone(),
                chain_id: v.chain_id,
            })
            .collect()
    } else {
        anyhow::bail!(
            "no chains configured and no --rpc/--chain provided. \
             Add chains with `cast-interop chains add` or pass --rpc."
        );
    };

    // Spawn one tokio task per chain (true parallel I/O).
    let mut handles = Vec::with_capacity(targets.len());
    for t in targets {
        let tokens_clone = token_addresses.clone();
        handles.push(tokio::spawn(query_chain(
            t.alias,
            t.rpc,
            t.chain_id,
            wallet,
            tokens_clone,
        )));
    }

    let mut results: Vec<ChainBalances> = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.context("chain query task panicked")?);
    }

    // Sort by chain name for deterministic output.
    results.sort_by(|a, b| a.chain.cmp(&b.chain));

    // Optionally hide chains where every balance is zero.
    if args.hide_zero {
        results.retain(|c| {
            c.error.is_some()
                || !c.native_wei.eq("0")
                || c.tokens.iter().any(|t| !t.raw.eq("0"))
        });
    }

    let output = BalancesOutput {
        address: format!("{wallet:#x}"),
        chains: results,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // ── Human-readable output ────────────────────────────────────────────────
    println!("Balances for {wallet:#x}");
    if output.chains.is_empty() {
        println!("  (no chains with non-zero balances)");
        return Ok(());
    }
    for chain in &output.chains {
        println!("\n  chain: {} (id={})", chain.chain, chain.chain_id);
        if let Some(err) = &chain.error {
            println!("    ⚠ error: {err}");
            continue;
        }
        println!(
            "    native: {} ETH  ({} wei)",
            chain.native_formatted, chain.native_wei
        );
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

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    // ── format_units ─────────────────────────────────────────────────────────

    #[test]
    fn format_units_zero() {
        assert_eq!(format_units(U256::ZERO, 18), "0");
    }

    #[test]
    fn format_units_no_decimals() {
        assert_eq!(format_units(U256::from(42u64), 0), "42");
    }

    #[test]
    fn format_units_whole_eth() {
        let one_eth = U256::from(1_000_000_000_000_000_000u64);
        assert_eq!(format_units(one_eth, 18), "1");
    }

    #[test]
    fn format_units_one_point_five_eth() {
        let val = U256::from(1_500_000_000_000_000_000u64);
        assert_eq!(format_units(val, 18), "1.5");
    }

    #[test]
    fn format_units_small_fraction() {
        // 1 wei
        assert_eq!(
            format_units(U256::from(1u64), 18),
            "0.000000000000000001"
        );
    }

    #[test]
    fn format_units_usdc_six_decimals() {
        // 1 USDC = 1_000_000
        assert_eq!(format_units(U256::from(1_000_000u64), 6), "1");
        // 1.5 USDC
        assert_eq!(format_units(U256::from(1_500_000u64), 6), "1.5");
        // 0.000001 USDC
        assert_eq!(format_units(U256::from(1u64), 6), "0.000001");
    }

    #[test]
    fn format_units_trims_trailing_zeros() {
        // 1.50 should be "1.5" not "1.50"
        let val = U256::from(1_500_000u64);
        assert_eq!(format_units(val, 6), "1.5");
    }

    // ── decode_abi_string ─────────────────────────────────────────────────────

    fn abi_encode_string(s: &str) -> Vec<u8> {
        // offset = 32, length = s.len(), data = s padded to 32-byte boundary
        let mut out = vec![0u8; 32]; // offset word = 0x20
        out[31] = 0x20;
        let len = s.len();
        let mut len_word = [0u8; 32];
        len_word[24..].copy_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&len_word);
        out.extend_from_slice(s.as_bytes());
        // pad to 32-byte boundary
        let pad = (32 - (len % 32)) % 32;
        out.extend(std::iter::repeat(0u8).take(pad));
        out
    }

    #[test]
    fn decode_abi_string_normal() {
        let raw = abi_encode_string("USDC");
        assert_eq!(decode_abi_string(&raw), "USDC");
    }

    #[test]
    fn decode_abi_string_long_symbol() {
        let raw = abi_encode_string("WBTC");
        assert_eq!(decode_abi_string(&raw), "WBTC");
    }

    #[test]
    fn decode_abi_string_bytes32_fallback() {
        // MKR-style: symbol encoded as right-padded bytes32 "MKR\0\0..."
        let mut raw = [0u8; 32];
        raw[..3].copy_from_slice(b"MKR");
        assert_eq!(decode_abi_string(&raw), "MKR");
    }

    #[test]
    fn decode_abi_string_empty_input() {
        assert_eq!(decode_abi_string(&[]), "???");
    }

    #[test]
    fn decode_abi_string_too_short() {
        assert_eq!(decode_abi_string(&[0u8; 10]), "???");
    }

    // ── ERC-20 selector encoding ──────────────────────────────────────────────

    #[test]
    fn selectors_are_correct() {
        // Verify the hardcoded 4-byte selectors match keccak256 of the function sig.
        // We check against known values rather than computing at test time.
        let balance_of = encode_balance_of(Address::ZERO);
        assert_eq!(&balance_of[..4], &[0x70u8, 0xa0, 0x82, 0x31]);

        let decimals = encode_decimals();
        assert_eq!(&decimals[..], &[0x31u8, 0x3c, 0xe5, 0x67]);

        let symbol = encode_symbol();
        assert_eq!(&symbol[..], &[0x95u8, 0xd8, 0x9b, 0x41]);
    }

    #[test]
    fn encode_balance_of_packs_address_correctly() {
        use std::str::FromStr;
        let addr = Address::from_str("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045").unwrap();
        let calldata = encode_balance_of(addr);
        assert_eq!(calldata.len(), 36); // 4 selector + 32 padded address
        // First 12 bytes of the address word should be zero (left-padded).
        assert!(calldata[4..16].iter().all(|&b| b == 0));
        // Last 20 bytes should be the address.
        assert_eq!(&calldata[16..36], addr.as_slice());
    }
}
