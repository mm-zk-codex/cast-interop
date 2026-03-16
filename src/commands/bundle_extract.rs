use crate::abi::{encode_interop_bundle, extract_bundles_from_receipt};
use crate::cli::BundleExtractArgs;
use crate::config::Config;
use crate::rpc::{get_transaction_receipt, RpcClient};
use crate::types::{format_hex, AddressBook, BundleExtractOutput};
use alloy_primitives::B256;
use anyhow::{Context, Result};
use std::fs;
use std::str::FromStr;

/// Extract an encoded bundle from an interop transaction.
///
/// Scans for InteropBundleSent logs and prints/writes the encoded bundle.
pub async fn run(args: BundleExtractArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&client, tx_hash).await?;

    let bundles = extract_bundles_from_receipt(&receipt)?;
    if bundles.is_empty() {
        anyhow::bail!("InteropBundleSent not found in receipt");
    }
    let idx = args.bundle_index as usize;
    if idx >= bundles.len() {
        anyhow::bail!(
            "bundle-index {} out of range (transaction has {} bundle(s))",
            idx,
            bundles.len()
        );
    }
    let detected = &bundles[idx];
    let bundle_hash = detected.bundle_hash;
    let bundle = &detected.bundle;

    let encoded = encode_interop_bundle(&bundle);
    let encoded_hex = format_hex(&encoded.0);
    let output = BundleExtractOutput {
        bundle_hash: format!("{bundle_hash:#x}"),
        encoded_bundle_hex: encoded_hex.clone(),
        bundle: crate::abi::bundle_view(&bundle),
    };

    println!("encodedBundleHex: {}", encoded_hex);
    println!("bundleHash: {bundle_hash:#x}");

    if let Some(path) = args.out {
        fs::write(path, encoded_hex)?;
    }
    if let Some(path) = args.json_out {
        fs::write(path, serde_json::to_string_pretty(&output)?)?;
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}
