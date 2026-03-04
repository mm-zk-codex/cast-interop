use crate::abi::encode_unbundle_bundle_call;
use crate::cli::UnbundleArgs;
use crate::commands::bundle_action::{
    decode_revert_reason, decode_send_transaction, load_hex_or_path,
};
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{require_signer_or_dry_run, AddressBook, InteropBundle};
use alloy_primitives::{Address, Bytes};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionInput;
use alloy_sol_types::SolValue;
use anyhow::{Context, Result};
use serde::Serialize;
use std::str::FromStr;

/// Status values for a single call inside a bundle.
///
/// Mirrors the `CallStatus` enum in the InteropHandler contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallStatus {
    Unprocessed = 0,
    Executed = 1,
    Cancelled = 2,
}

impl CallStatus {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

impl std::fmt::Display for CallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallStatus::Unprocessed => write!(f, "unprocessed"),
            CallStatus::Executed => write!(f, "executed"),
            CallStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for CallStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "unprocessed" | "0" => Ok(CallStatus::Unprocessed),
            "executed" | "1" => Ok(CallStatus::Executed),
            "cancelled" | "canceled" | "2" => Ok(CallStatus::Cancelled),
            other => anyhow::bail!(
                "unknown call status '{other}' — expected: unprocessed, executed, cancelled"
            ),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UnbundleOutput {
    handler: String,
    source_chain_id: String,
    call_count: usize,
    call_statuses: Vec<String>,
    dry_run: bool,
    tx_hash: Option<String>,
}

/// Execute the `bundle unbundle` subcommand.
///
/// Calls `unbundleBundle` on the InteropHandler, allowing individual calls in a bundle
/// to be marked as `Executed` or `Cancelled` independently. Useful when only a subset
/// of a bundle's calls should proceed, or to cancel stuck calls after verification.
pub async fn run(args: UnbundleArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let handler = args
        .handler
        .as_deref()
        .map(|v| Address::from_str(v))
        .transpose()
        .context("invalid handler address")?
        .unwrap_or(addresses.interop_handler);

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;

    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "bundle unbundle")?;

    let encoded_bundle = load_hex_or_path(&args.bundle)?;

    // Decode the bundle to validate call count before hitting the network.
    let bundle =
        InteropBundle::abi_decode(&encoded_bundle).context("failed to decode bundle bytes")?;

    let call_statuses: Vec<CallStatus> = args
        .call_statuses
        .split(',')
        .map(|s| s.parse::<CallStatus>())
        .collect::<Result<Vec<_>>>()
        .context("invalid --call-statuses")?;

    if call_statuses.len() != bundle.calls.len() {
        anyhow::bail!(
            "--call-statuses has {} entries but the bundle has {} call(s)",
            call_statuses.len(),
            bundle.calls.len()
        );
    }

    let source_chain_id = config.resolve_chain_id(&args.source_chain_id)?;
    let status_bytes: Vec<u8> = call_statuses.iter().map(|s| s.as_u8()).collect();

    let calldata =
        encode_unbundle_bundle_call(source_chain_id, Bytes::from(encoded_bundle), status_bytes);

    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;

    let status_strings: Vec<String> = call_statuses.iter().map(|s| s.to_string()).collect();

    if args.dry_run {
        match eth_call(&client, handler, calldata).await {
            Ok(_) => println!("dry-run success"),
            Err(err) => {
                if let Some(reason) = decode_revert_reason(err.to_string()) {
                    println!("dry-run revert: {reason}");
                } else {
                    println!("dry-run failed: {err}");
                }
            }
        }
        if args.json {
            let output = UnbundleOutput {
                handler: format!("{handler:#x}"),
                source_chain_id: source_chain_id.to_string(),
                call_count: call_statuses.len(),
                call_statuses: status_strings,
                dry_run: true,
                tx_hash: None,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        return Ok(());
    }

    let wallet = wallet.expect("wallet required");
    let chain_id = client.provider.get_chain_id().await?;

    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .with_chain_id(chain_id)
        .connect(&resolved.url)
        .await?;

    let request = alloy_rpc_types::TransactionRequest {
        to: Some(alloy_primitives::TxKind::Call(handler)),
        input: TransactionInput::new(calldata),
        ..Default::default()
    };

    let pending = decode_send_transaction(provider.send_transaction(request).await)?;
    let hash = pending.tx_hash();
    let tx_hash = format!("{hash:#x}");
    println!("sent tx: {tx_hash}");

    if args.json {
        let output = UnbundleOutput {
            handler: format!("{handler:#x}"),
            source_chain_id: source_chain_id.to_string(),
            call_count: call_statuses.len(),
            call_statuses: status_strings,
            dry_run: false,
            tx_hash: Some(tx_hash),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "unbundleBundle sent: {} call(s) → [{}]",
            call_statuses.len(),
            status_strings.join(", ")
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Bytes, U256};

    #[test]
    fn test_parse_call_status_words() {
        assert_eq!(
            "executed".parse::<CallStatus>().unwrap(),
            CallStatus::Executed
        );
        assert_eq!(
            "cancelled".parse::<CallStatus>().unwrap(),
            CallStatus::Cancelled
        );
        assert_eq!(
            "canceled".parse::<CallStatus>().unwrap(),
            CallStatus::Cancelled
        );
        assert_eq!(
            "unprocessed".parse::<CallStatus>().unwrap(),
            CallStatus::Unprocessed
        );
    }

    #[test]
    fn test_parse_call_status_numbers() {
        assert_eq!("0".parse::<CallStatus>().unwrap(), CallStatus::Unprocessed);
        assert_eq!("1".parse::<CallStatus>().unwrap(), CallStatus::Executed);
        assert_eq!("2".parse::<CallStatus>().unwrap(), CallStatus::Cancelled);
    }

    #[test]
    fn test_parse_call_status_case_insensitive() {
        assert_eq!(
            "EXECUTED".parse::<CallStatus>().unwrap(),
            CallStatus::Executed
        );
        assert_eq!(
            "Cancelled".parse::<CallStatus>().unwrap(),
            CallStatus::Cancelled
        );
    }

    #[test]
    fn test_parse_call_status_invalid() {
        assert!("invalid".parse::<CallStatus>().is_err());
        assert!("3".parse::<CallStatus>().is_err());
    }

    #[test]
    fn test_call_status_as_u8() {
        assert_eq!(CallStatus::Unprocessed.as_u8(), 0);
        assert_eq!(CallStatus::Executed.as_u8(), 1);
        assert_eq!(CallStatus::Cancelled.as_u8(), 2);
    }

    #[test]
    fn test_encode_unbundle_call_produces_non_empty_bytes() {
        use crate::abi::encode_unbundle_bundle_call;
        let result = encode_unbundle_bundle_call(
            U256::from(270u64),
            Bytes::from(vec![0xde, 0xad, 0xbe, 0xef]),
            vec![1u8, 2u8],
        );
        // Should have 4-byte selector + ABI-encoded params
        assert!(result.len() > 4);
    }
}
