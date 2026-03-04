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
    /// Mark this chain as an Ethereum L1 chain.
    /// When true, zkSync-specific RPC methods (zks_*) are not expected and are skipped.
    #[serde(rename = "isL1", skip_serializing_if = "Option::is_none")]
    pub is_l1: Option<bool>,
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
    /// True when the chain is configured as an Ethereum L1 (no zks_* methods available).
    pub is_l1: bool,
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
                is_l1: false,
            });
        }

        if let Some(alias) = chain {
            if let Some(chain_cfg) = self.chains.as_ref().and_then(|chains| chains.get(alias)) {
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.to_string()),
                    chain_id: chain_cfg.chain_id,
                    is_l1: chain_cfg.is_l1.unwrap_or(false),
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
                        is_l1: false,
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
                    is_l1: chain_cfg.is_l1.unwrap_or(false),
                });
            }
            if chains.len() == 1 {
                let (alias, chain_cfg) = chains.iter().next().expect("non-empty");
                return Ok(ResolvedRpc {
                    url: chain_cfg.rpc.clone(),
                    alias: Some(alias.clone()),
                    chain_id: chain_cfg.chain_id,
                    is_l1: chain_cfg.is_l1.unwrap_or(false),
                });
            }
        }
        if let Some(default) = self.rpc.as_ref().and_then(|cfg| cfg.default.clone()) {
            return Ok(ResolvedRpc {
                url: default,
                alias: Some("default".to_string()),
                chain_id: None,
                is_l1: false,
            });
        }
        anyhow::bail!("no rpc configured (set --rpc or --chain, or configure a default)")
    }

    pub fn set_chain(&mut self, alias: String, rpc: String, chain_id: u64, is_l1: bool) {
        let chains = self.chains.get_or_insert_with(BTreeMap::new);
        chains.insert(
            alias,
            ChainConfig {
                rpc,
                chain_id: Some(chain_id),
                is_l1: if is_l1 { Some(true) } else { None },
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

fn default_config_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("cast-interop").join("config.toml");
    }
    PathBuf::from("./config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config_with_chain(alias: &str, is_l1: bool) -> Config {
        let mut config = Config::default();
        config.set_chain(
            alias.to_string(),
            "http://localhost:8545".to_string(),
            1,
            is_l1,
        );
        config
    }

    #[test]
    fn test_resolve_rpc_l2_chain_is_not_l1() {
        let config = make_config_with_chain("era", false);
        let resolved = config.resolve_rpc(None, Some("era")).unwrap();
        assert!(!resolved.is_l1);
        assert_eq!(resolved.url, "http://localhost:8545");
    }

    #[test]
    fn test_resolve_rpc_l1_chain_sets_is_l1() {
        let config = make_config_with_chain("eth", true);
        let resolved = config.resolve_rpc(None, Some("eth")).unwrap();
        assert!(resolved.is_l1);
    }

    #[test]
    fn test_resolve_rpc_bare_url_is_not_l1() {
        let config = Config::default();
        let resolved = config
            .resolve_rpc(Some("http://localhost:9000"), None)
            .unwrap();
        assert!(!resolved.is_l1);
    }

    #[test]
    fn test_chain_config_serialization_omits_is_l1_when_false() {
        let chain = ChainConfig {
            rpc: "http://localhost:8545".to_string(),
            chain_id: Some(1),
            is_l1: None,
        };
        let serialized = toml::to_string(&chain).unwrap();
        assert!(!serialized.contains("isL1"));
    }

    #[test]
    fn test_chain_config_serialization_includes_is_l1_when_true() {
        let chain = ChainConfig {
            rpc: "http://localhost:8545".to_string(),
            chain_id: Some(1),
            is_l1: Some(true),
        };
        let serialized = toml::to_string(&chain).unwrap();
        assert!(serialized.contains("isL1"));
    }

    #[test]
    fn test_set_chain_stores_is_l1() {
        let mut config = Config::default();
        config.set_chain(
            "eth".to_string(),
            "http://localhost:8545".to_string(),
            1,
            true,
        );
        let chain = config.chain("eth").unwrap();
        assert_eq!(chain.is_l1, Some(true));
    }

    #[test]
    fn test_set_chain_omits_is_l1_when_false() {
        let mut config = Config::default();
        config.set_chain(
            "era".to_string(),
            "http://localhost:8545".to_string(),
            324,
            false,
        );
        let chain = config.chain("era").unwrap();
        assert_eq!(chain.is_l1, None);
    }
}
