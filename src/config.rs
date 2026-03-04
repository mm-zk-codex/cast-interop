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
#[allow(dead_code)]
pub struct ResolvedRpc {
    pub url: String,
    pub alias: Option<String>,
    pub chain_id: Option<u64>,
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
            });
        }

        if let Some(alias) = chain {
            if let Some(chain_cfg) = self.chains.as_ref().and_then(|chains| chains.get(alias)) {
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.to_string()),
                    chain_id: chain_cfg.chain_id,
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
                });
            }
            if chains.len() == 1 {
                let (alias, chain_cfg) = chains.iter().next().expect("non-empty");
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.clone()),
                    chain_id: chain_cfg.chain_id,
                });
            }
        }
        if let Some(default) = self.rpc.as_ref().and_then(|cfg| cfg.default.clone()) {
            return Ok(ResolvedRpc {
                url: default,
                alias: Some("default".to_string()),
                chain_id: None,
            });
        }
        anyhow::bail!("no rpc configured (set --rpc or --chain, or configure a default)")
    }

    pub fn set_chain(&mut self, alias: String, rpc: String, chain_id: u64) {
        let chains = self.chains.get_or_insert_with(BTreeMap::new);
        chains.insert(
            alias,
            ChainConfig {
                rpc,
                chain_id: Some(chain_id),
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn config_with_chain(alias: &str, rpc: &str, chain_id: Option<u64>) -> Config {
        let mut chains = BTreeMap::new();
        chains.insert(
            alias.to_string(),
            ChainConfig {
                rpc: rpc.to_string(),
                chain_id,
            },
        );
        Config {
            chains: Some(chains),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_rpc_explicit_url() {
        let config = Config::default();
        let resolved = config
            .resolve_rpc(Some("http://localhost:8545"), None)
            .unwrap();
        assert_eq!(resolved.url, "http://localhost:8545");
        assert!(resolved.alias.is_none());
        assert!(resolved.chain_id.is_none());
    }

    #[test]
    fn resolve_rpc_both_rpc_and_chain_errors() {
        let config = Config::default();
        assert!(config.resolve_rpc(Some("http://x"), Some("alias")).is_err());
    }

    #[test]
    fn resolve_rpc_unknown_alias_errors() {
        let config = Config::default();
        assert!(config.resolve_rpc(None, Some("unknown")).is_err());
    }

    #[test]
    fn resolve_rpc_no_args_no_config_errors() {
        let config = Config::default();
        assert!(config.resolve_rpc(None, None).is_err());
    }

    #[test]
    fn resolve_rpc_known_chain_alias() {
        let config = config_with_chain("mychain", "http://localhost:3050", Some(270));
        let resolved = config.resolve_rpc(None, Some("mychain")).unwrap();
        assert_eq!(resolved.url, "http://localhost:3050");
        assert_eq!(resolved.alias.as_deref(), Some("mychain"));
        assert_eq!(resolved.chain_id, Some(270));
    }

    #[test]
    fn resolve_rpc_single_chain_fallback() {
        let config = config_with_chain("only", "http://localhost:4000", Some(1));
        let resolved = config.resolve_rpc(None, None).unwrap();
        assert_eq!(resolved.url, "http://localhost:4000");
    }

    #[test]
    fn resolve_rpc_default_chain_alias_fallback() {
        let config = config_with_chain("default", "http://default-rpc", Some(1));
        let resolved = config.resolve_rpc(None, None).unwrap();
        assert_eq!(resolved.url, "http://default-rpc");
        assert_eq!(resolved.alias.as_deref(), Some("default"));
    }

    #[test]
    fn resolve_rpc_prefers_default_chain_over_single_chain() {
        let mut config = config_with_chain("default", "http://default-rpc", Some(1));
        config
            .chains
            .as_mut()
            .unwrap()
            .insert("other".to_string(), ChainConfig {
                rpc: "http://other-rpc".to_string(),
                chain_id: Some(2),
            });
        let resolved = config.resolve_rpc(None, None).unwrap();
        assert_eq!(resolved.url, "http://default-rpc");
    }

    #[test]
    fn resolve_rpc_multiple_chains_no_default_errors() {
        let mut config = config_with_chain("a", "http://a", Some(1));
        config
            .chains
            .as_mut()
            .unwrap()
            .insert("b".to_string(), ChainConfig {
                rpc: "http://b".to_string(),
                chain_id: Some(2),
            });
        assert!(config.resolve_rpc(None, None).is_err());
    }

    #[test]
    fn resolve_chain_id_numeric_string() {
        let config = Config::default();
        assert_eq!(
            config.resolve_chain_id("270").unwrap(),
            alloy_primitives::U256::from(270u64)
        );
    }

    #[test]
    fn resolve_chain_id_from_alias() {
        let config = config_with_chain("era", "http://rpc", Some(324));
        assert_eq!(
            config.resolve_chain_id("era").unwrap(),
            alloy_primitives::U256::from(324u64)
        );
    }

    #[test]
    fn resolve_chain_id_alias_without_chain_id_errors() {
        let config = config_with_chain("era", "http://rpc", None);
        assert!(config.resolve_chain_id("era").is_err());
    }

    #[test]
    fn resolve_chain_id_invalid_string_errors() {
        let config = Config::default();
        assert!(config.resolve_chain_id("notanumber").is_err());
    }

    #[test]
    fn abi_dir_default_is_deps() {
        assert_eq!(Config::default().abi_dir(), PathBuf::from("./deps"));
    }

    #[test]
    fn abi_dir_custom_path() {
        let config = Config {
            abi: Some(AbiConfig {
                dir: Some(PathBuf::from("/custom/abi")),
            }),
            ..Default::default()
        };
        assert_eq!(config.abi_dir(), PathBuf::from("/custom/abi"));
    }

    #[test]
    fn signer_env_default_is_private_key() {
        assert_eq!(Config::default().signer_env(), "PRIVATE_KEY");
    }

    #[test]
    fn signer_env_custom_name() {
        let config = Config {
            signer: Some(SignerConfig {
                private_key_env: Some("MY_KEY".to_string()),
            }),
            ..Default::default()
        };
        assert_eq!(config.signer_env(), "MY_KEY");
    }

    #[test]
    fn set_chain_and_get_chain_roundtrip() {
        let mut config = Config::default();
        config.set_chain("test".to_string(), "http://test".to_string(), 99);
        let chain = config.chain("test").unwrap();
        assert_eq!(chain.rpc, "http://test");
        assert_eq!(chain.chain_id, Some(99));
    }

    #[test]
    fn remove_chain_existing_returns_true() {
        let mut config = Config::default();
        config.set_chain("test".to_string(), "http://test".to_string(), 99);
        assert!(config.remove_chain("test"));
        assert!(config.chain("test").is_none());
    }

    #[test]
    fn remove_chain_nonexistent_returns_false() {
        let mut config = Config::default();
        assert!(!config.remove_chain("nonexistent"));
    }

    #[test]
    fn set_chain_overwrites_existing() {
        let mut config = Config::default();
        config.set_chain("test".to_string(), "http://old".to_string(), 1);
        config.set_chain("test".to_string(), "http://new".to_string(), 2);
        let chain = config.chain("test").unwrap();
        assert_eq!(chain.rpc, "http://new");
        assert_eq!(chain.chain_id, Some(2));
    }
}

fn default_config_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("cast-interop").join("config.toml");
    }
    PathBuf::from("./config.toml")
}
