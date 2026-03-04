use crate::abi::{decode_interop_bundle_sent, interop_bundle_sent_topic};
use crate::cli::BundleScanArgs;
use crate::config::Config;
use crate::rpc::{raw_rpc, RpcClient};
use crate::types::{b256_to_hex, u256_to_string, AddressBook};
use alloy_primitives::Bytes;
use alloy_provider::Provider;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// A single InteropBundleSent event decoded from a log.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannedBundle {
    tx_hash: String,
    block_number: u64,
    bundle_hash: String,
    l2l1_msg_hash: String,
    source_chain_id: String,
    destination_chain_id: String,
    call_count: usize,
}

/// Raw log entry from eth_getLogs JSON-RPC response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLog {
    transaction_hash: Option<String>,
    block_number: Option<String>,
    data: String,
}

/// Scan recent blocks for InteropBundleSent events emitted by the InteropCenter.
///
/// Lets you discover bundles without knowing a specific transaction hash.
pub async fn run(args: BundleScanArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;

    let latest_block = client.provider.get_block_number().await?;
    let from_block = args.from_block.unwrap_or_else(|| latest_block.saturating_sub(args.lookback));
    let to_block = args.to_block.unwrap_or(latest_block);

    if args.verbose {
        eprintln!(
            "Scanning blocks {} to {} for InteropBundleSent events on {}...",
            from_block, to_block, resolved.url
        );
    }

    let center_addr = format!("{:#x}", addresses.interop_center);
    let topic = format!("{:#x}", interop_bundle_sent_topic());

    let filter = json!([{
        "fromBlock": format!("0x{:x}", from_block),
        "toBlock":   format!("0x{:x}", to_block),
        "address":   center_addr,
        "topics":    [topic],
    }]);

    let logs: Vec<RawLog> = raw_rpc(&client, "eth_getLogs", filter).await?;

    let mut results: Vec<ScannedBundle> = Vec::new();

    for log in &logs {
        let raw = hex::decode(log.data.trim_start_matches("0x"))?;
        let data = Bytes::from(raw);
        let (l2l1_hash, bundle_hash, bundle) = match decode_interop_bundle_sent(data) {
            Ok(v) => v,
            Err(err) => {
                if args.verbose {
                    eprintln!("warning: failed to decode log: {err}");
                }
                continue;
            }
        };

        let dest_chain = u256_to_string(bundle.destinationChainId);
        if let Some(ref filter_chain) = args.dest_chain_id {
            if &dest_chain != filter_chain {
                continue;
            }
        }

        let block_number = log
            .block_number
            .as_deref()
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);

        results.push(ScannedBundle {
            tx_hash: log.transaction_hash.clone().unwrap_or_default(),
            block_number,
            bundle_hash: b256_to_hex(bundle_hash),
            l2l1_msg_hash: b256_to_hex(l2l1_hash),
            source_chain_id: u256_to_string(bundle.sourceChainId),
            destination_chain_id: dest_chain,
            call_count: bundle.calls.len(),
        });
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    if results.is_empty() {
        println!(
            "No InteropBundleSent events found in blocks {}..{}",
            from_block, to_block
        );
        return Ok(());
    }

    println!(
        "Found {} bundle(s) in blocks {}..{}:\n",
        results.len(),
        from_block,
        to_block
    );
    for r in &results {
        println!("  block:       {}", r.block_number);
        println!("  tx:          {}", r.tx_hash);
        println!("  bundleHash:  {}", r.bundle_hash);
        println!(
            "  route:       chain {} -> chain {}",
            r.source_chain_id, r.destination_chain_id
        );
        println!("  calls:       {}", r.call_count);
        println!();
    }

    Ok(())
}
