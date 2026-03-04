use crate::cli::{ChainsAddArgs, ChainsListArgs, ChainsRemoveArgs, ChainsValidateArgs};
use crate::config::{ChainConfig, Config};
use crate::rpc::{raw_rpc, RpcClient};
use crate::types::AddressBook;
use alloy_provider::Provider;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChainListItem {
    alias: String,
    rpc: String,
    chain_id: Option<String>,
}

/// List configured chain aliases and their RPC URLs.
pub async fn run_list(args: ChainsListArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let mut items = Vec::new();

    let mut chains = config.chains.clone().unwrap_or_default();
    if chains.is_empty() {
        chains = legacy_chains(&config);
    }

    for (alias, cfg) in chains {
        let chain_id = probe_chain_id(&cfg).await.ok().or(cfg.chain_id);
        items.push(ChainListItem {
            alias,
            rpc: redact_url(&cfg.rpc),
            chain_id: chain_id.map(|id| id.to_string()),
        });
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if items.is_empty() {
        println!("no chains configured");
        return Ok(());
    }

    println!("{:<12} {:<10} {}", "alias", "chainId", "rpc");
    for item in items {
        let chain_id = item.chain_id.unwrap_or_else(|| "unknown".to_string());
        println!("{:<12} {:<10} {}", item.alias, chain_id, item.rpc);
    }

    Ok(())
}

/// Add a chain alias by probing the chain ID from the RPC URL.
pub async fn run_add(
    args: ChainsAddArgs,
    mut config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let rpc = args.rpc.trim();
    let client = RpcClient::new(rpc).await?;
    let chain_id = client
        .provider
        .get_chain_id()
        .await
        .context("failed to fetch eth_chainId")?;
    let chain_id = u64::try_from(chain_id).map_err(|_| anyhow!("chainId too large"))?;

    config.set_chain(args.alias.clone(), rpc.to_string(), chain_id);
    config.save()?;

    println!(
        "added chain {alias} (chainId {chain_id})",
        alias = args.alias
    );
    Ok(())
}

/// Remove a chain alias from the configuration file.
pub async fn run_remove(
    args: ChainsRemoveArgs,
    mut config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    if !config.remove_chain(&args.alias) {
        anyhow::bail!("chain alias not found: {}", args.alias);
    }
    config.save()?;
    println!("removed chain {}", args.alias);
    Ok(())
}

/// Build a map of legacy chain entries from deprecated config fields.
fn legacy_chains(config: &Config) -> BTreeMap<String, ChainConfig> {
    let mut map = BTreeMap::new();
    if let Some(rpc) = &config.rpc {
        if let Some(url) = &rpc.default {
            map.insert(
                "default".to_string(),
                ChainConfig {
                    rpc: url.clone(),
                    chain_id: None,
                },
            );
        }
        if let Some(url) = &rpc.a {
            map.insert(
                "a".to_string(),
                ChainConfig {
                    rpc: url.clone(),
                    chain_id: None,
                },
            );
        }
        if let Some(url) = &rpc.b {
            map.insert(
                "b".to_string(),
                ChainConfig {
                    rpc: url.clone(),
                    chain_id: None,
                },
            );
        }
    }
    map
}

/// Probe the chain ID from an RPC URL for display purposes.
async fn probe_chain_id(cfg: &ChainConfig) -> Result<u64> {
    let client = RpcClient::new(&cfg.rpc).await?;
    let chain = client.provider.get_chain_id().await?;
    Ok(chain)
}

// ── chains validate ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidateCheck {
    chain: String,
    name: String,
    status: String,
    details: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

/// Validate one or all configured chain aliases against their live RPC endpoints.
///
/// Checks performed per chain:
/// 1. RPC reachability
/// 2. Stored chainId vs live chainId (mismatch = misconfiguration)
/// 3. zkSync `zks_getL2ToL1LogProof` support (needed for bundle relaying)
/// 4. zkSync `zks_getL1BatchNumber` support (needed for proof fetching)
pub async fn run_validate(
    args: ChainsValidateArgs,
    config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let mut chains = config.chains.clone().unwrap_or_default();
    if chains.is_empty() {
        chains = legacy_chains(&config);
    }

    // Filter to the requested alias, or validate all.
    let targets: BTreeMap<String, ChainConfig> = match &args.alias {
        Some(alias) => {
            let cfg = chains
                .get(alias)
                .ok_or_else(|| anyhow!("chain alias not found: {alias}"))?
                .clone();
            let mut m = BTreeMap::new();
            m.insert(alias.clone(), cfg);
            m
        }
        None => {
            if chains.is_empty() {
                println!("no chains configured");
                return Ok(());
            }
            chains
        }
    };

    let mut all_checks: Vec<ValidateCheck> = Vec::new();
    for (alias, cfg) in &targets {
        let checks = validate_chain(alias, cfg).await;
        all_checks.extend(checks);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&all_checks)?);
        return Ok(());
    }

    // Human-readable grouped output
    let mut current_chain = String::new();
    for check in &all_checks {
        if check.chain != current_chain {
            current_chain = check.chain.clone();
            println!("\nchain: {current_chain}");
        }
        let icon = match check.status.as_str() {
            "ok" => "  ✅",
            "warn" => "  ⚠️ ",
            "fail" => "  ❌",
            _ => "  • ",
        };
        println!("{icon} {}: {}", check.name, check.details);
        if let Some(hint) = &check.hint {
            println!("       hint: {hint}");
        }
    }

    // Summary
    let fails = all_checks.iter().filter(|c| c.status == "fail").count();
    let warns = all_checks.iter().filter(|c| c.status == "warn").count();
    println!(
        "\n{} chain(s) validated — {} failure(s), {} warning(s)",
        targets.len(),
        fails,
        warns
    );

    Ok(())
}

/// Run all validation checks for a single chain alias.
async fn validate_chain(alias: &str, cfg: &ChainConfig) -> Vec<ValidateCheck> {
    let mut checks = Vec::new();

    // ── 1. RPC reachability ──────────────────────────────────────────────────
    let client = match RpcClient::new(&cfg.rpc).await {
        Ok(c) => {
            checks.push(ValidateCheck {
                chain: alias.to_string(),
                name: "rpc_reachable".to_string(),
                status: "ok".to_string(),
                details: "RPC reachable".to_string(),
                hint: None,
            });
            c
        }
        Err(err) => {
            checks.push(ValidateCheck {
                chain: alias.to_string(),
                name: "rpc_reachable".to_string(),
                status: "fail".to_string(),
                details: format!("RPC not reachable: {err}"),
                hint: Some("Check the RPC URL or network connectivity.".to_string()),
            });
            // Cannot continue without a client.
            return checks;
        }
    };

    // ── 2. Live chainId vs stored chainId ───────────────────────────────────
    match client.provider.get_chain_id().await {
        Ok(live_id) => match cfg.chain_id {
            Some(stored_id) if stored_id != live_id => {
                checks.push(ValidateCheck {
                        chain: alias.to_string(),
                        name: "chain_id_match".to_string(),
                        status: "fail".to_string(),
                        details: format!(
                            "stored chainId {stored_id} does not match live chainId {live_id}"
                        ),
                        hint: Some(format!(
                            "Run: cast-interop chains rm {alias} && cast-interop chains add {alias} --rpc <URL>"
                        )),
                    });
            }
            Some(stored_id) => {
                checks.push(ValidateCheck {
                    chain: alias.to_string(),
                    name: "chain_id_match".to_string(),
                    status: "ok".to_string(),
                    details: format!("chainId {stored_id} matches live RPC"),
                    hint: None,
                });
            }
            None => {
                checks.push(ValidateCheck {
                        chain: alias.to_string(),
                        name: "chain_id_match".to_string(),
                        status: "warn".to_string(),
                        details: format!("no stored chainId; live RPC reports {live_id}"),
                        hint: Some(format!(
                            "Re-add the chain to store the chainId: cast-interop chains add {alias} --rpc <URL>"
                        )),
                    });
            }
        },
        Err(err) => {
            checks.push(ValidateCheck {
                chain: alias.to_string(),
                name: "chain_id_match".to_string(),
                status: "fail".to_string(),
                details: format!("eth_chainId failed: {err}"),
                hint: Some("Ensure the RPC URL points to an EVM-compatible endpoint.".to_string()),
            });
        }
    }

    // ── 3. zks_getL2ToL1LogProof support ────────────────────────────────────
    let proof_result = raw_rpc::<serde_json::Value>(
        &client,
        "zks_getL2ToL1LogProof",
        json!([
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            0
        ]),
    )
    .await;
    match proof_result {
        Ok(_) => checks.push(ValidateCheck {
            chain: alias.to_string(),
            name: "zks_log_proof".to_string(),
            status: "ok".to_string(),
            details: "zks_getL2ToL1LogProof supported".to_string(),
            hint: None,
        }),
        Err(err) => {
            let msg = err.to_string();
            let (status, hint) = if msg.contains("Method not found")
                || msg.contains("method not found")
            {
                (
                    "fail",
                    Some("This RPC does not support zks_getL2ToL1LogProof — bundle relaying will not work.".to_string()),
                )
            } else {
                (
                    "warn",
                    Some(
                        "Log proof check returned an error; the method may still be supported."
                            .to_string(),
                    ),
                )
            };
            checks.push(ValidateCheck {
                chain: alias.to_string(),
                name: "zks_log_proof".to_string(),
                status: status.to_string(),
                details: format!("zks_getL2ToL1LogProof: {msg}"),
                hint,
            });
        }
    }

    // ── 4. zks_getL1BatchNumber support ─────────────────────────────────────
    let batch_result =
        raw_rpc::<serde_json::Value>(&client, "zks_getL1BatchNumber", json!([])).await;
    match batch_result {
        Ok(_) => checks.push(ValidateCheck {
            chain: alias.to_string(),
            name: "zks_batch_number".to_string(),
            status: "ok".to_string(),
            details: "zks_getL1BatchNumber supported".to_string(),
            hint: None,
        }),
        Err(err) => {
            let msg = err.to_string();
            let (status, hint) = if msg.contains("Method not found")
                || msg.contains("method not found")
            {
                (
                    "fail",
                    Some("This RPC does not support zks_getL1BatchNumber — proof fetching will not work.".to_string()),
                )
            } else {
                (
                    "warn",
                    Some(
                        "Batch number check returned an error; the method may still be supported."
                            .to_string(),
                    ),
                )
            };
            checks.push(ValidateCheck {
                chain: alias.to_string(),
                name: "zks_batch_number".to_string(),
                status: status.to_string(),
                details: format!("zks_getL1BatchNumber: {msg}"),
                hint,
            });
        }
    }

    checks
}

/// Redact credentials from a URL string for display.
fn redact_url(value: &str) -> String {
    match url::Url::parse(value) {
        Ok(mut parsed) => {
            let has_user = !parsed.username().is_empty();
            let has_password = parsed.password().is_some();
            if has_user {
                let _ = parsed.set_username("REDACTED");
            }
            if has_password {
                let _ = parsed.set_password(Some("REDACTED"));
            }
            parsed.to_string()
        }
        Err(_) => value.to_string(),
    }
}
