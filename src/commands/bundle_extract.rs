use crate::cli::BundleExtractArgs;
use crate::commands::relay::extract_bundles;
use crate::config::Config;
use crate::rpc::{get_transaction_receipt, RpcClient};
use crate::types::{format_hex, AddressBook, BundleExtractOutput};
use alloy_primitives::B256;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::str::FromStr;

/// Extract encoded bundle(s) from an interop transaction.
///
/// With `--all`, extracts every bundle emitted by the transaction.
/// With `--msg-index`, extracts the bundle at the specified index.
/// Default: extracts the first bundle (msg_index=0).
pub async fn run(args: BundleExtractArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    let tx_hash =
        B256::from_str(&args.tx).with_context(|| format!("invalid tx hash {}", args.tx))?;
    let receipt = get_transaction_receipt(&client, tx_hash).await?;

    let all_bundles = extract_bundles(&receipt, addresses.interop_center)?;
    if all_bundles.is_empty() {
        anyhow::bail!("no InteropBundleSent events found in transaction {}", args.tx);
    }

    let selected = if args.all {
        all_bundles
    } else {
        let target = args.msg_index.unwrap_or(0);
        let bundle = all_bundles
            .into_iter()
            .find(|b| b.msg_index == target)
            .ok_or_else(|| {
                anyhow!(
                    "no bundle with msg_index={target} in tx {tx_hash:#x} \
                     (use --all to extract every bundle)"
                )
            })?;
        vec![bundle]
    };

    for detected in &selected {
        let encoded_hex = format_hex(&detected.encoded_bundle.0);
        let bundle_view = crate::abi::bundle_view_from_encoded(&detected.encoded_bundle)?;
        let output = BundleExtractOutput {
            bundle_hash: format!("{:#x}", detected.bundle_hash),
            encoded_bundle_hex: encoded_hex.clone(),
            bundle: bundle_view,
        };

        if args.json {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            if selected.len() > 1 {
                println!("--- msg_index={} ---", detected.msg_index);
            }
            println!("encodedBundleHex: {encoded_hex}");
            println!("bundleHash: {:#x}", detected.bundle_hash);
        }

        if let Some(ref path) = args.out {
            let file_path = if selected.len() > 1 {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                path.with_file_name(format!("{stem}_{}{ext}", detected.msg_index))
            } else {
                path.clone()
            };
            fs::write(file_path, &encoded_hex)?;
        }
        if let Some(ref path) = args.json_out {
            let file_path = if selected.len() > 1 {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                path.with_file_name(format!("{stem}_{}{ext}", detected.msg_index))
            } else {
                path.clone()
            };
            fs::write(file_path, serde_json::to_string_pretty(&output)?)?;
        }
    }

    Ok(())
}
