use crate::cli::ExplainArgs;
use crate::config::Config;
use crate::encode::decode_evm_v1_address;
use crate::rpc::RpcClient;
use crate::signer::{load_signer, signer_address, SignerOptions};
use crate::types::{AddressBook, MessageInclusionProof};
use alloy_dyn_abi::SolType;
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::Provider;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExplainItem {
    check: String,
    status: String,
    details: String,
}

/// Explain why a bundle proof would succeed or fail.
///
/// Performs checks on sender, chain IDs, and permissions for the signer.
pub async fn run(args: ExplainArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;
    let chain_id = client.provider.get_chain_id().await?;

    let bundle_bytes = load_hex_or_path(&args.bundle)?;
    let bundle: crate::types::InteropBundle =
        crate::types::InteropBundle::abi_decode(&bundle_bytes)
            .context("failed to decode bundle")?;
    let proof = load_proof(&args.proof)?;

    let signer = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;

    let mut checks = Vec::new();
    checks.push(check_sender(&proof, addresses.interop_center));
    checks.push(check_message_prefix(&proof));
    checks.push(check_destination_chain(&bundle, chain_id));
    checks.push(check_source_chain(&bundle, &proof));

    if let Some(signer) = signer {
        let signer_addr = signer_address(&signer)?;
        checks.push(check_permissions(
            &bundle,
            signer_addr,
            chain_id,
            "executionAddress",
            |b| &b.bundleAttributes.executionAddress,
        ));
        checks.push(check_permissions(
            &bundle,
            signer_addr,
            chain_id,
            "unbundlerAddress",
            |b| &b.bundleAttributes.unbundlerAddress,
        ));
    } else {
        checks.push(ExplainItem {
            check: "permissions".to_string(),
            status: "warn".to_string(),
            details: "signer not provided; skipping permission checks".to_string(),
        });
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
        return Ok(());
    }

    for check in checks {
        let icon = match check.status.as_str() {
            "ok" => "✅",
            "warn" => "⚠️",
            "fail" => "❌",
            _ => "•",
        };
        println!("{icon} {}: {}", check.check, check.details);
    }
    Ok(())
}

/// Check whether the proof sender matches the interop center.
fn check_sender(proof: &MessageInclusionProof, center: Address) -> ExplainItem {
    let expected = format!("{center:#x}").to_lowercase();
    let actual = proof.message.sender.to_lowercase();
    if actual == expected {
        ExplainItem {
            check: "proof.sender".to_string(),
            status: "ok".to_string(),
            details: "proof sender matches interop center".to_string(),
        }
    } else {
        ExplainItem {
            check: "proof.sender".to_string(),
            status: "fail".to_string(),
            details: format!("proof sender {actual} does not match center {expected}"),
        }
    }
}

/// Ensure the proof message data has the bundle prefix.
fn check_message_prefix(proof: &MessageInclusionProof) -> ExplainItem {
    if proof.message.data.to_lowercase().starts_with("0x01") {
        ExplainItem {
            check: "proof.message.data".to_string(),
            status: "ok".to_string(),
            details: "message data has bundle prefix 0x01".to_string(),
        }
    } else {
        ExplainItem {
            check: "proof.message.data".to_string(),
            status: "fail".to_string(),
            details: "message data missing 0x01 bundle prefix".to_string(),
        }
    }
}

/// Verify the bundle destination chain matches the current chain.
fn check_destination_chain(bundle: &crate::types::InteropBundle, chain_id: u64) -> ExplainItem {
    if bundle.destinationChainId == U256::from(chain_id) {
        ExplainItem {
            check: "bundle.destinationChainId".to_string(),
            status: "ok".to_string(),
            details: "bundle destination matches current chain".to_string(),
        }
    } else {
        ExplainItem {
            check: "bundle.destinationChainId".to_string(),
            status: "fail".to_string(),
            details: format!(
                "bundle destination {destination} does not match current chain {chain_id}",
                destination = bundle.destinationChainId
            ),
        }
    }
}

/// Verify the bundle source chain matches the proof chain ID.
fn check_source_chain(
    bundle: &crate::types::InteropBundle,
    proof: &MessageInclusionProof,
) -> ExplainItem {
    let proof_chain = proof.chain_id.clone();
    let bundle_chain = bundle.sourceChainId.to_string();
    if bundle_chain == proof_chain {
        ExplainItem {
            check: "bundle.sourceChainId".to_string(),
            status: "ok".to_string(),
            details: "bundle source matches proof chainId".to_string(),
        }
    } else {
        ExplainItem {
            check: "bundle.sourceChainId".to_string(),
            status: "fail".to_string(),
            details: format!(
                "bundle source {bundle_chain} does not match proof chainId {proof_chain}"
            ),
        }
    }
}

/// Verify execution/unbundler permissions for the signer.
fn check_permissions<F>(
    bundle: &crate::types::InteropBundle,
    signer: Address,
    chain_id: u64,
    label: &str,
    accessor: F,
) -> ExplainItem
where
    F: Fn(&crate::types::InteropBundle) -> &Bytes,
{
    let bytes = accessor(bundle);
    if bytes.is_empty() {
        return ExplainItem {
            check: label.to_string(),
            status: "ok".to_string(),
            details: format!("{label} is permissionless"),
        };
    }
    match decode_evm_v1_address(bytes) {
        Ok((addr_chain_id, addr)) => {
            let addr = addr.unwrap_or(Address::ZERO);
            let valid_chain = addr_chain_id == U256::ZERO || addr_chain_id == U256::from(chain_id);
            if valid_chain && addr == signer {
                ExplainItem {
                    check: label.to_string(),
                    status: "ok".to_string(),
                    details: format!("{label} allows signer {signer:#x}"),
                }
            } else {
                ExplainItem {
                    check: label.to_string(),
                    status: "fail".to_string(),
                    details: format!(
                        "{label} does not allow signer {signer:#x} (chainId {addr_chain_id}, addr {addr:#x})"
                    ),
                }
            }
        }
        Err(err) => ExplainItem {
            check: label.to_string(),
            status: "warn".to_string(),
            details: format!("failed to decode {label}: {err}"),
        },
    }
}

/// Load a hex string from inline input or a file path.
fn load_hex_or_path(value: &str) -> Result<Vec<u8>> {
    if Path::new(value).exists() {
        let contents = fs::read_to_string(value)?;
        return decode_hex(&contents);
    }
    decode_hex(value)
}

/// Decode a hex string, stripping a 0x prefix if present.
fn decode_hex(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex::decode(raw).map_err(|err| anyhow!("invalid hex {value}: {err}"))
}

/// Load a MessageInclusionProof from JSON or a file path.
fn load_proof(value: &str) -> Result<MessageInclusionProof> {
    if Path::new(value).exists() {
        let contents = fs::read_to_string(value)?;
        return serde_json::from_str(&contents).context("invalid proof json");
    }
    if value.trim_start().starts_with('{') {
        return serde_json::from_str(value).context("invalid proof json");
    }
    anyhow::bail!("proof must be a JSON string or path")
}
