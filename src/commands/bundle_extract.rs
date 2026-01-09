use crate::abi::{decode_interop_bundle_sent, encode_interop_bundle, interop_bundle_sent_topic};
use crate::cli::BundleExtractArgs;
use crate::config::Config;
use crate::rpc::{get_transaction_receipt, RpcClient};
use crate::types::{format_hex, AddressBook, BundleExtractOutput};
use alloy_primitives::B256;
use anyhow::{Context, Result};
use std::fs;
use std::str::FromStr;

pub async fn run(args: BundleExtractArgs, _config: Config, _addresses: AddressBook) -> Result<()> {
    let client = RpcClient::new(&args.rpc).await?;
    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&client, tx_hash).await?;

    let mut found = None;
    for log in receipt.logs() {
        if log.topics().first().copied() == Some(interop_bundle_sent_topic()) {
            let decoded = decode_interop_bundle_sent(log.data().data.clone())?;
            found = Some(decoded);
            break;
        }
    }

    let Some((_, bundle_hash, bundle)) = found else {
        anyhow::bail!("InteropBundleSent not found in receipt");
    };

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

    Ok(())
}
