use anyhow::{anyhow, Context, Result};
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
            if let Some(chain_cfg) = self
                .chains
                .as_ref()
                .and_then(|chains| chains.get(alias))
            {
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

fn default_config_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("cast-interop").join("config.toml");
    }
    PathBuf::from("./config.toml")
}
