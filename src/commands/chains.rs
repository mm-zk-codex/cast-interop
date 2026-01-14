use crate::cli::{ChainsAddArgs, ChainsListArgs, ChainsRemoveArgs};
use crate::config::{ChainConfig, Config};
use crate::rpc::RpcClient;
use crate::types::AddressBook;
use alloy_provider::Provider;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
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
