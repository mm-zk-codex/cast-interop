use crate::abi::{
    decode_bundle_status, decode_call_status, encode_bundle_status_call, encode_call_status_call,
};
use crate::cli::StatusArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{bytes_from_hex, parse_b256, AddressBook, CallStatusView, StatusOutput};
use alloy_primitives::U256;
use alloy_sol_types::SolValue;
use anyhow::Result;

pub async fn run(args: StatusArgs, _config: Config, addresses: AddressBook) -> Result<()> {
    let client = RpcClient::new(&args.rpc)?;
    let bundle_hash = parse_b256(&args.bundle_hash)?;
    let call = encode_bundle_status_call(bundle_hash);
    let result = eth_call(&client, addresses.interop_handler, call).await?;
    let status_value = decode_bundle_status(result)?;
    let bundle_status = bundle_status_string(status_value);

    let calls = if let Some(bundle_hex) = args.bundle.as_deref() {
        let bytes = load_hex_or_path(bundle_hex)?;
        let bundle: crate::types::InteropBundle =
            crate::types::InteropBundle::abi_decode(&bytes, true)?;
        let mut statuses = Vec::new();
        for (idx, _) in bundle.calls.iter().enumerate() {
            let call = encode_call_status_call(bundle_hash, U256::from(idx));
            let data = eth_call(&client, addresses.interop_handler, call).await?;
            let status = decode_call_status(data)?;
            statuses.push(CallStatusView {
                index: idx as u64,
                status: call_status_string(status),
            });
        }
        Some(statuses)
    } else {
        None
    };

    let output = StatusOutput {
        bundle_hash: format!("{bundle_hash:#x}"),
        bundle_status: bundle_status.clone(),
        calls: calls.clone(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("bundleHash: {bundle_hash:#x}");
    println!("bundleStatus: {bundle_status}");
    if let Some(call_statuses) = calls {
        for call in call_statuses {
            println!(
                "call[{index}] {status}",
                index = call.index,
                status = call.status
            );
        }
    }
    Ok(())
}

fn load_hex_or_path(value: &str) -> Result<Vec<u8>> {
    if std::path::Path::new(value).exists() {
        let contents = std::fs::read_to_string(value)?;
        return bytes_from_hex(&contents).map(|bytes| bytes.0.to_vec());
    }
    bytes_from_hex(value).map(|bytes| bytes.0.to_vec())
}

fn bundle_status_string(value: u8) -> String {
    match value {
        0 => "Unreceived",
        1 => "Verified",
        2 => "FullyExecuted",
        3 => "Unbundled",
        _ => "Unknown",
    }
    .to_string()
}

fn call_status_string(value: u8) -> String {
    match value {
        0 => "Unprocessed",
        1 => "Executed",
        2 => "Cancelled",
        _ => "Unknown",
    }
    .to_string()
}
