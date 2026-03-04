use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub rpc: Option<RpcConfig>,
    pub chains: Option<BTreeMap<String, ChainConfig>>,
    pub addresses: Option<AddressConfig>,
    pub abi: Option<AbiConfig>,
    pub signer: Option<SignerConfig>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rpc: None,
            chains: None,
            addresses: None,
            abi: None,
            signer: None,
            path: PathBuf::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct RpcConfig {
    pub default: Option<String>,
    pub a: Option<String>,
    pub b: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct ChainConfig {
    pub rpc: String,
    #[serde(rename = "chainId")]
    pub chain_id: Option<u64>,
    /// Base URL of the Prividium permissions API (e.g. `https://permissions.example.com`).
    /// When set, the CLI authenticates via SIWE before making RPC requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prividium_url: Option<String>,
    /// Name of the environment variable holding the Prividium auth private key.
    /// Defaults to `PRIVIDIUM_PRIVATE_KEY` when `prividium_url` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prividium_key_env: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct AddressConfig {
    pub interop_center: Option<String>,
    pub interop_handler: Option<String>,
    pub interop_root_storage: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct AbiConfig {
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct SignerConfig {
    pub private_key_env: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRpc {
    pub url: String,
    pub alias: Option<String>,
    pub chain_id: Option<u64>,
    /// Set when the chain requires Prividium authentication.
    pub prividium_url: Option<String>,
    pub prividium_key_env: Option<String>,
}

impl ResolvedRpc {
    /// Build an [`crate::rpc::RpcClient`] for this endpoint.
    ///
    /// If the chain has a `prividium_url`, performs SIWE authentication first
    /// and injects the resulting bearer token as an `Authorization` header.
    /// Otherwise returns a plain unauthenticated client.
    pub async fn to_rpc_client(&self) -> Result<crate::rpc::RpcClient> {
        if let Some(prividium_url) = &self.prividium_url {
            let key = crate::prividium::resolve_private_key(None, self.prividium_key_env.as_deref())?;
            let session = crate::prividium::authenticate(prividium_url, &key).await?;
            crate::rpc::RpcClient::new_with_auth(&self.url, &session.token)
        } else {
            crate::rpc::RpcClient::new(&self.url).await
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = match path {
            Some(path) => path.to_path_buf(),
            None => default_config_path(),
        };

        if !path.exists() {
            let mut config = Self::default();
            config.path = path;
            return Ok(config);
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config: Config = toml::from_str(&contents)
            .with_context(|| format!("failed to parse config {}", path.display()))?;
        config.path = path;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = if self.path.as_os_str().is_empty() {
            default_config_path()
        } else {
            self.path.clone()
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(&self)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    pub fn abi_dir(&self) -> PathBuf {
        if let Some(abi) = &self.abi {
            if let Some(dir) = &abi.dir {
                return dir.clone();
            }
        }
        PathBuf::from("./deps")
    }

    pub fn signer_env(&self) -> String {
        self.signer
            .as_ref()
            .and_then(|cfg| cfg.private_key_env.clone())
            .unwrap_or_else(|| "PRIVATE_KEY".to_string())
    }

    pub fn resolve_rpc(&self, rpc: Option<&str>, chain: Option<&str>) -> Result<ResolvedRpc> {
        if rpc.is_some() && chain.is_some() {
            anyhow::bail!("cannot set both --rpc and --chain");
        }

        if let Some(rpc) = rpc {
            return Ok(ResolvedRpc {
                url: rpc.to_string(),
                alias: None,
                chain_id: None,
                prividium_url: None,
                prividium_key_env: None,
            });
        }

        if let Some(alias) = chain {
            if let Some(chain_cfg) = self.chains.as_ref().and_then(|chains| chains.get(alias)) {
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.to_string()),
                    chain_id: chain_cfg.chain_id,
                    prividium_url: chain_cfg.prividium_url.clone(),
                    prividium_key_env: chain_cfg.prividium_key_env.clone(),
                });
            }
            if let Some(legacy) = self.rpc.as_ref() {
                let url = match alias {
                    "default" => legacy.default.clone(),
                    "a" => legacy.a.clone(),
                    "b" => legacy.b.clone(),
                    _ => None,
                };
                if let Some(url) = url {
                    return Ok(ResolvedRpc {
                        url,
                        alias: Some(alias.to_string()),
                        chain_id: None,
                        prividium_url: None,
                        prividium_key_env: None,
                    });
                }
            }
            anyhow::bail!("unknown chain alias: {alias}");
        }

        if let Some(chains) = self.chains.as_ref() {
            if let Some(chain_cfg) = chains.get("default") {
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some("default".to_string()),
                    chain_id: chain_cfg.chain_id,
                    prividium_url: chain_cfg.prividium_url.clone(),
                    prividium_key_env: chain_cfg.prividium_key_env.clone(),
                });
            }
            if chains.len() == 1 {
                let (alias, chain_cfg) = chains.iter().next().expect("non-empty");
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.clone()),
                    chain_id: chain_cfg.chain_id,
                    prividium_url: chain_cfg.prividium_url.clone(),
                    prividium_key_env: chain_cfg.prividium_key_env.clone(),
                });
            }
        }
        if let Some(default) = self.rpc.as_ref().and_then(|cfg| cfg.default.clone()) {
            return Ok(ResolvedRpc {
                url: default,
                alias: Some("default".to_string()),
                chain_id: None,
                prividium_url: None,
                prividium_key_env: None,
            });
        }
        anyhow::bail!("no rpc configured (set --rpc or --chain, or configure a default)")
    }

    pub fn set_chain(&mut self, alias: String, cfg: ChainConfig) {
        self.chains
            .get_or_insert_with(BTreeMap::new)
            .insert(alias, cfg);
    }

    pub fn remove_chain(&mut self, alias: &str) -> bool {
        self.chains
            .as_mut()
            .and_then(|chains| chains.remove(alias))
            .is_some()
    }

    pub fn chain(&self, alias: &str) -> Option<&ChainConfig> {
        self.chains.as_ref()?.get(alias)
    }

    pub fn resolve_chain_id(&self, value: &str) -> Result<alloy_primitives::U256> {
        if let Some(chain) = self.chain(value) {
            if let Some(chain_id) = chain.chain_id {
                return Ok(alloy_primitives::U256::from(chain_id));
            }
            anyhow::bail!("chainId missing for alias {value}");
        }
        crate::types::parse_u256(value)
    }
}

fn default_config_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("cast-interop").join("config.toml");
    }
    PathBuf::from("./config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chain(rpc: &str, chain_id: Option<u64>) -> ChainConfig {
        ChainConfig {
            rpc: rpc.to_string(),
            chain_id,
            prividium_url: None,
            prividium_key_env: None,
        }
    }

    fn make_prividium_chain(rpc: &str, chain_id: u64, priv_url: &str, key_env: &str) -> ChainConfig {
        ChainConfig {
            rpc: rpc.to_string(),
            chain_id: Some(chain_id),
            prividium_url: Some(priv_url.to_string()),
            prividium_key_env: Some(key_env.to_string()),
        }
    }

    // --- set_chain / remove_chain / chain ---

    #[test]
    fn set_and_retrieve_chain() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", Some(324)));
        let chain = config.chain("era").expect("chain should exist");
        assert_eq!(chain.rpc, "https://mainnet.era.zksync.io");
        assert_eq!(chain.chain_id, Some(324));
    }

    #[test]
    fn set_chain_overwrites_existing() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://old.url", Some(1)));
        config.set_chain("era".to_string(), make_chain("https://new.url", Some(324)));
        assert_eq!(config.chain("era").unwrap().rpc, "https://new.url");
    }

    #[test]
    fn remove_chain_returns_true_when_present() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", Some(324)));
        assert!(config.remove_chain("era"));
        assert!(config.chain("era").is_none());
    }

    #[test]
    fn remove_chain_returns_false_when_absent() {
        let mut config = Config::default();
        assert!(!config.remove_chain("nonexistent"));
    }

    #[test]
    fn chain_returns_none_for_missing_alias() {
        let config = Config::default();
        assert!(config.chain("missing").is_none());
    }

    // --- resolve_rpc ---

    #[test]
    fn resolve_rpc_with_explicit_url() {
        let config = Config::default();
        let resolved = config.resolve_rpc(Some("https://custom.rpc"), None).unwrap();
        assert_eq!(resolved.url, "https://custom.rpc");
        assert!(resolved.prividium_url.is_none());
    }

    #[test]
    fn resolve_rpc_with_chain_alias() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", Some(324)));
        let resolved = config.resolve_rpc(None, Some("era")).unwrap();
        assert_eq!(resolved.url, "https://mainnet.era.zksync.io");
        assert_eq!(resolved.chain_id, Some(324));
        assert_eq!(resolved.alias.as_deref(), Some("era"));
    }

    #[test]
    fn resolve_rpc_prividium_chain_propagates_auth_fields() {
        let mut config = Config::default();
        config.set_chain(
            "mychain".to_string(),
            make_prividium_chain(
                "https://priv.rpc/rpc",
                270,
                "https://permissions.example.com",
                "MY_KEY_ENV",
            ),
        );
        let resolved = config.resolve_rpc(None, Some("mychain")).unwrap();
        assert_eq!(resolved.url, "https://priv.rpc/rpc");
        assert_eq!(resolved.prividium_url.as_deref(), Some("https://permissions.example.com"));
        assert_eq!(resolved.prividium_key_env.as_deref(), Some("MY_KEY_ENV"));
    }

    #[test]
    fn resolve_rpc_both_rpc_and_chain_errors() {
        let config = Config::default();
        let err = config
            .resolve_rpc(Some("https://custom.rpc"), Some("era"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot set both"), "got: {err}");
    }

    #[test]
    fn resolve_rpc_unknown_alias_errors() {
        let config = Config::default();
        let err = config.resolve_rpc(None, Some("unknown")).unwrap_err().to_string();
        assert!(err.contains("unknown chain alias"), "got: {err}");
    }

    #[test]
    fn resolve_rpc_uses_default_alias_when_no_args() {
        let mut config = Config::default();
        config.set_chain("default".to_string(), make_chain("https://default.rpc", Some(1)));
        let resolved = config.resolve_rpc(None, None).unwrap();
        assert_eq!(resolved.url, "https://default.rpc");
    }

    #[test]
    fn resolve_rpc_uses_sole_chain_when_no_args() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", Some(324)));
        let resolved = config.resolve_rpc(None, None).unwrap();
        assert_eq!(resolved.url, "https://mainnet.era.zksync.io");
    }

    #[test]
    fn resolve_rpc_no_config_errors() {
        let config = Config::default();
        let err = config.resolve_rpc(None, None).unwrap_err().to_string();
        assert!(err.contains("no rpc configured"), "got: {err}");
    }

    // --- resolve_chain_id ---

    #[test]
    fn resolve_chain_id_from_alias() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", Some(324)));
        let id = config.resolve_chain_id("era").unwrap();
        assert_eq!(id, alloy_primitives::U256::from(324u64));
    }

    #[test]
    fn resolve_chain_id_alias_missing_chain_id_errors() {
        let mut config = Config::default();
        config.set_chain("era".to_string(), make_chain("https://mainnet.era.zksync.io", None));
        let err = config.resolve_chain_id("era").unwrap_err().to_string();
        assert!(err.contains("chainId missing"), "got: {err}");
    }

    #[test]
    fn resolve_chain_id_from_numeric_string() {
        let config = Config::default();
        let id = config.resolve_chain_id("324").unwrap();
        assert_eq!(id, alloy_primitives::U256::from(324u64));
    }

    // --- ChainConfig serialization (TOML round-trip) ---

    #[test]
    fn chain_config_toml_round_trip_without_prividium() {
        let cfg = make_chain("https://mainnet.era.zksync.io", Some(324));
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        // prividium fields should not appear when None
        assert!(!toml_str.contains("prividium"), "got: {toml_str}");
        let parsed: ChainConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.rpc, cfg.rpc);
        assert_eq!(parsed.chain_id, cfg.chain_id);
    }

    #[test]
    fn chain_config_toml_round_trip_with_prividium() {
        let cfg = make_prividium_chain(
            "https://priv.rpc/rpc",
            270,
            "https://permissions.example.com",
            "PRIV_KEY_ENV",
        );
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        assert!(toml_str.contains("prividium_url"), "got: {toml_str}");
        assert!(toml_str.contains("prividium_key_env"), "got: {toml_str}");
        let parsed: ChainConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.prividium_url.as_deref(), Some("https://permissions.example.com"));
        assert_eq!(parsed.prividium_key_env.as_deref(), Some("PRIV_KEY_ENV"));
    }
}
