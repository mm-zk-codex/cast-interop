use crate::abi::{
    bundle_executed_topic, bundle_unbundled_topic, bundle_verified_topic, call_processed_topic,
    decode_interop_bundle_sent, decode_message_sent, decode_u8, interop_bundle_sent_topic,
    l1_message_sent_topic, message_sent_topic,
};
use crate::cli::TxShowArgs;
use crate::config::Config;
use crate::rpc::{get_transaction_receipt, RpcClient};
use crate::types::{
    address_to_hex, b256_to_hex, format_hex, u256_to_string, AddressBook, EventView,
    InteropBundleView, TxShowOutput, INTEROP_CENTER_ADDRESS, L1_SENDER_ADDRESS,
};
use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result};
use serde_json::json;
use std::str::FromStr;

/// Decode interop events from a transaction receipt.
///
/// Prints bundle information, message hashes, and event summaries.
pub async fn run(args: TxShowArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    let tx_hash = B256::from_str(&args.tx_hash)
        .with_context(|| format!("invalid tx hash {}", args.tx_hash))?;
    let receipt = get_transaction_receipt(&client, tx_hash).await?;

    let mut bundle_view: Option<InteropBundleView> = None;
    let mut bundle_hash: Option<String> = None;
    let mut l2l1_msg_hash: Option<String> = None;
    let mut events = Vec::new();

    for log in receipt.logs() {
        let topic0 = log.topics().get(0).cloned();
        let Some(topic0) = topic0 else { continue };
        if topic0 == interop_bundle_sent_topic() && log.address() == INTEROP_CENTER_ADDRESS {
            let (l2l1_hash, interop_hash, bundle) =
                decode_interop_bundle_sent(log.data().data.clone())?;
            let bundle_json = crate::abi::bundle_view(&bundle);
            bundle_view = Some(bundle_json.clone());
            bundle_hash = Some(b256_to_hex(interop_hash));
            l2l1_msg_hash = Some(b256_to_hex(l2l1_hash));
            events.push(EventView {
                name: "InteropBundleSent".to_string(),
                address: address_to_hex(log.address()),
                data: serde_json::to_value(&bundle_json)?,
            });
        } else if topic0 == l1_message_sent_topic() && log.address() == L1_SENDER_ADDRESS {
            print!("Decoding L1MessageSent event...\n");
            let sender = log
                .topics()
                .get(1)
                .map(|topic| address_to_hex(Address::from_slice(&topic.as_slice()[12..])))
                .unwrap_or_default();
            let l2l1_msg_hash = log
                .topics()
                .get(2)
                .map(|topic| b256_to_hex(*topic))
                .unwrap_or_default();
            events.push(EventView {
                name: "L1MessageSent".to_string(),
                address: sender.clone(),
                data: json!({
                    "sender": sender,
                    "l2l1MsgHash": l2l1_msg_hash,
                    "payload": format_hex(log.data().data.as_ref()),
                }),
            });
        } else if topic0 == message_sent_topic() && log.address() != INTEROP_CENTER_ADDRESS {
            let decoded = decode_message_sent(log.data().data.clone())?;
            let send_id = log
                .topics()
                .get(1)
                .map(|topic| b256_to_hex(*topic))
                .unwrap_or_default();
            events.push(EventView {
                name: "MessageSent".to_string(),
                address: address_to_hex(log.address()),
                data: json!({
                    "sendId": send_id,
                    "sender": format_hex(decoded.sender.as_ref()),
                    "recipient": format_hex(decoded.recipient.as_ref()),
                    "payload": format_hex(decoded.payload.as_ref()),
                    "value": u256_to_string(decoded.value),
                    "attributes": decoded.attributes.iter().map(|attr| format_hex(attr.as_ref())).collect::<Vec<_>>(),
                }),
            });
        } else if topic0 == bundle_verified_topic() {
            events.push(simple_bundle_event("BundleVerified", &log));
        } else if topic0 == bundle_executed_topic() {
            events.push(simple_bundle_event("BundleExecuted", &log));
        } else if topic0 == bundle_unbundled_topic() {
            events.push(simple_bundle_event("BundleUnbundled", &log));
        } else if topic0 == call_processed_topic() {
            let bundle_hash = log
                .topics()
                .get(1)
                .map(|topic| b256_to_hex(*topic))
                .unwrap_or_default();
            let call_index = log
                .topics()
                .get(2)
                .map(|topic| U256::from_be_slice(topic.as_slice()))
                .map(u256_to_string)
                .unwrap_or_default();
            let status = decode_u8(log.data().data.clone())?;
            events.push(EventView {
                name: "CallProcessed".to_string(),
                address: address_to_hex(log.address()),
                data: json!({
                    "bundleHash": bundle_hash,
                    "callIndex": call_index,
                    "status": status,
                }),
            });
        } else if !args.interop_only {
            continue;
        }
    }

    let output = TxShowOutput {
        tx_hash: format!("{tx_hash:#x}"),
        bundle: bundle_view.clone(),
        bundle_hash: bundle_hash.clone(),
        l2l1_msg_hash: l2l1_msg_hash.clone(),
        interop_events: events.clone(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("tx: {:#x}", tx_hash);
    if let Some(bundle_hash) = bundle_hash {
        println!("bundleHash: {bundle_hash}");
    }
    if let Some(l2l1_msg_hash) = l2l1_msg_hash {
        println!("l2l1MsgHash: {l2l1_msg_hash}");
    }
    if let Some(bundle) = bundle_view {
        println!(
            "bundle: sourceChainId={} destinationChainId={} calls={}",
            bundle.source_chain_id,
            bundle.destination_chain_id,
            bundle.calls.len()
        );
        for (idx, call) in bundle.calls.iter().enumerate() {
            println!(
                "  call[{idx}] to={} from={} value={} data_len={}",
                call.to,
                call.from,
                call.value,
                (call.data.len().saturating_sub(2)) / 2
            );
        }
        println!(
            "bundleAttributes: executionAddress={} unbundlerAddress={}",
            bundle.bundle_attributes.execution_address, bundle.bundle_attributes.unbundler_address
        );
    }
    if !events.is_empty() {
        println!("events:");
        for event in events {
            println!("  {} @ {}", event.name, event.address);
        }
    }
    Ok(())
}

/// Render a minimal bundle event for verified/executed/unbundled logs.
fn simple_bundle_event(name: &str, log: &alloy_rpc_types::Log) -> EventView {
    let bundle_hash = log
        .topics()
        .get(1)
        .map(|topic| b256_to_hex(*topic))
        .unwrap_or_default();
    EventView {
        name: name.to_string(),
        address: address_to_hex(log.address()),
        data: json!({ "bundleHash": bundle_hash }),
    }
}
