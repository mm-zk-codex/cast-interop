use crate::cli::{ChainsAddArgs, ChainsListArgs, ChainsRemoveArgs};
use crate::config::{ChainConfig, Config, ResolvedRpc};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    prividium_url: Option<String>,
}

/// List configured chain aliases and their RPC URLs.
pub async fn run_list(args: ChainsListArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let mut items = Vec::new();

    let mut chains = config.chains.clone().unwrap_or_default();
    if chains.is_empty() {
        chains = legacy_chains(&config);
    }

    for (alias, cfg) in chains {
        // Use the cached chain_id if available; only probe if missing.
        let chain_id = if cfg.chain_id.is_some() {
            cfg.chain_id
        } else {
            probe_chain_id(&cfg).await.ok()
        };
        items.push(ChainListItem {
            alias,
            rpc: redact_url(&cfg.rpc),
            chain_id: chain_id.map(|id| id.to_string()),
            prividium_url: cfg.prividium_url.clone(),
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

    println!("{:<12} {:<10} {:<8} {}", "alias", "chainId", "prividium", "rpc");
    for item in items {
        let chain_id = item.chain_id.unwrap_or_else(|| "unknown".to_string());
        let prividium = if item.prividium_url.is_some() { "yes" } else { "no" };
        println!("{:<12} {:<10} {:<8} {}", item.alias, chain_id, prividium, item.rpc);
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

    // Build a temporary ResolvedRpc so we can use the shared auth logic to
    // probe the chain ID (important when adding a Prividium chain).
    let resolved = ResolvedRpc {
        url: rpc.to_string(),
        alias: None,
        chain_id: None,
        prividium_url: args.prividium_url.clone(),
        prividium_key_env: args.prividium_key_env.clone(),
    };

    let client = resolved.to_rpc_client().await?;
    let chain_id = client
        .provider
        .get_chain_id()
        .await
        .context("failed to fetch eth_chainId")?;
    let chain_id = u64::try_from(chain_id).map_err(|_| anyhow!("chainId too large"))?;

    config.set_chain(
        args.alias.clone(),
        ChainConfig {
            rpc: rpc.to_string(),
            chain_id: Some(chain_id),
            prividium_url: args.prividium_url,
            prividium_key_env: args.prividium_key_env,
        },
    );
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
                    prividium_url: None,
                    prividium_key_env: None,
                },
            );
        }
        if let Some(url) = &rpc.a {
            map.insert(
                "a".to_string(),
                ChainConfig {
                    rpc: url.clone(),
                    chain_id: None,
                    prividium_url: None,
                    prividium_key_env: None,
                },
            );
        }
        if let Some(url) = &rpc.b {
            map.insert(
                "b".to_string(),
                ChainConfig {
                    rpc: url.clone(),
                    chain_id: None,
                    prividium_url: None,
                    prividium_key_env: None,
                },
            );
        }
    }
    map
}

/// Probe the chain ID from a ChainConfig for display purposes.
/// Uses Prividium auth if the chain has it configured.
async fn probe_chain_id(cfg: &ChainConfig) -> Result<u64> {
    let resolved = ResolvedRpc {
        url: cfg.rpc.clone(),
        alias: None,
        chain_id: None,
        prividium_url: cfg.prividium_url.clone(),
        prividium_key_env: cfg.prividium_key_env.clone(),
    };
    let client = resolved.to_rpc_client().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_plain_url_unchanged() {
        // url::Url::parse normalizes by adding a trailing slash to bare host URLs,
        // so we compare with the normalized form.
        let url = "https://mainnet.era.zksync.io";
        let redacted = redact_url(url);
        assert!(!redacted.contains("REDACTED"), "plain URL should not be redacted, got: {redacted}");
        assert!(redacted.starts_with("https://mainnet.era.zksync.io"), "got: {redacted}");
    }

    #[test]
    fn redact_url_with_user_and_password() {
        let url = "https://user:secret@rpc.example.com/endpoint";
        let redacted = redact_url(url);
        assert!(!redacted.contains("secret"), "password should be redacted, got: {redacted}");
        assert!(!redacted.contains("user"), "username should be redacted, got: {redacted}");
        assert!(redacted.contains("REDACTED"), "should contain REDACTED, got: {redacted}");
    }

    #[test]
    fn redact_url_with_user_only() {
        let url = "https://apikey@rpc.example.com";
        let redacted = redact_url(url);
        assert!(!redacted.contains("apikey"), "should not contain apikey, got: {redacted}");
        assert!(redacted.contains("REDACTED"), "should contain REDACTED, got: {redacted}");
    }

    #[test]
    fn redact_url_with_api_key_in_path() {
        // API key in path (not credentials) should NOT be redacted — only auth fields
        let url = "https://rpc.example.com/v1/abc123secretkey";
        let redacted = redact_url(url);
        assert_eq!(redacted, url);
    }

    #[test]
    fn redact_invalid_url_passes_through() {
        let not_a_url = "not-a-url-at-all";
        assert_eq!(redact_url(not_a_url), not_a_url);
    }
}
