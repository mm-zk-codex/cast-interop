use crate::cli::DoctorArgs;
use crate::config::Config;
use crate::rpc::{get_finalized_block_number, raw_rpc, RpcClient};
use crate::types::{address_to_hex, AddressBook};
use alloy_provider::Provider;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorCheck {
    name: String,
    status: String,
    details: String,
    hint: Option<String>,
}

pub async fn run(args: DoctorArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;

    let mut checks = Vec::new();
    let client = match RpcClient::new(&resolved.url).await {
        Ok(client) => {
            checks.push(DoctorCheck {
                name: "rpc_reachable".to_string(),
                status: "ok".to_string(),
                details: "RPC reachable".to_string(),
                hint: None,
            });
            client
        }
        Err(err) => {
            checks.push(DoctorCheck {
                name: "rpc_reachable".to_string(),
                status: "fail".to_string(),
                details: format!("RPC not reachable: {err}"),
                hint: Some("Check the RPC URL or network connectivity.".to_string()),
            });
            return output_checks(args.json, checks);
        }
    };

    match client.provider.get_chain_id().await {
        Ok(chain_id) => checks.push(DoctorCheck {
            name: "eth_chainId".to_string(),
            status: "ok".to_string(),
            details: format!("chainId {chain_id}"),
            hint: None,
        }),
        Err(err) => checks.push(DoctorCheck {
            name: "eth_chainId".to_string(),
            status: "fail".to_string(),
            details: format!("eth_chainId failed: {err}"),
            hint: Some("Ensure the RPC URL points to an EVM-compatible endpoint.".to_string()),
        }),
    };

    match get_finalized_block_number(&client).await {
        Ok(block) => checks.push(DoctorCheck {
            name: "finalized_block".to_string(),
            status: "ok".to_string(),
            details: format!("finalized block {block}"),
            hint: None,
        }),
        Err(err) => checks.push(DoctorCheck {
            name: "finalized_block".to_string(),
            status: "warn".to_string(),
            details: format!("finalized block not supported: {err}"),
            hint: Some("Use a zkSync RPC or one that supports finalized blocks.".to_string()),
        }),
    };

    let proof_check = raw_rpc::<serde_json::Value>(
        &client,
        "zks_getL2ToL1LogProof",
        json!([
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            0
        ]),
    )
    .await;
    match proof_check {
        Ok(_) => checks.push(DoctorCheck {
            name: "get_log_proof".to_string(),
            status: "ok".to_string(),
            details: "zks_getL2ToL1LogProof reachable".to_string(),
            hint: None,
        }),
        Err(err) => {
            let message = err.to_string();
            let status =
                if message.contains("Method not found") || message.contains("method not found") {
                    "warn"
                } else {
                    "warn"
                };
            checks.push(DoctorCheck {
                name: "get_log_proof".to_string(),
                status: status.to_string(),
                details: format!("log proof call failed: {message}"),
                hint: Some("RPC must support zks_getL2ToL1LogProof to fetch proofs.".to_string()),
            });
        }
    }

    checks
        .extend(check_contract("interop_center", addresses.interop_center, &client, &config).await);
    checks.extend(
        check_contract(
            "interop_handler",
            addresses.interop_handler,
            &client,
            &config,
        )
        .await,
    );
    checks.extend(
        check_contract(
            "interop_root_storage",
            addresses.interop_root_storage,
            &client,
            &config,
        )
        .await,
    );

    output_checks(args.json, checks)
}

async fn check_contract(
    name: &str,
    address: alloy_primitives::Address,
    client: &RpcClient,
    config: &Config,
) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let code = client.provider.get_code_at(address).await;
    match code {
        Ok(code) => {
            if code.is_empty() {
                checks.push(DoctorCheck {
                    name: format!("{name}_code"),
                    status: "fail".to_string(),
                    details: format!("{name} not deployed at {}", address_to_hex(address)),
                    hint: Some("Check address overrides or network configuration.".to_string()),
                });
            } else {
                checks.push(DoctorCheck {
                    name: format!("{name}_code"),
                    status: "ok".to_string(),
                    details: format!("{name} deployed at {}", address_to_hex(address)),
                    hint: None,
                });
            }
        }
        Err(err) => {
            checks.push(DoctorCheck {
                name: format!("{name}_code"),
                status: "warn".to_string(),
                details: format!("failed to check code for {name}: {err}"),
                hint: None,
            });
        }
    }

    let abi_dir = config.abi_dir();
    let abi_name = match name {
        "interop_center" => "InteropCenter.json",
        "interop_handler" => "InteropHandler.json",
        "interop_root_storage" => "MessageVerification.json",
        _ => "unknown",
    };
    let abi_path = abi_dir.join(abi_name);
    if abi_path.exists() {
        checks.push(DoctorCheck {
            name: format!("{name}_abi"),
            status: "ok".to_string(),
            details: format!("ABI found at {}", abi_path.display()),
            hint: None,
        });
    } else {
        checks.push(DoctorCheck {
            name: format!("{name}_abi"),
            status: "warn".to_string(),
            details: format!("ABI missing: {}", abi_path.display()),
            hint: Some("Ensure ABI files are present in the abi directory.".to_string()),
        });
    }

    checks
}

fn output_checks(json: bool, checks: Vec<DoctorCheck>) -> Result<()> {
    if json {
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
        println!("{icon} {}: {}", check.name, check.details);
        if let Some(hint) = check.hint {
            println!("  hint: {hint}");
        }
    }
    Ok(())
}
