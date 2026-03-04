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

fn default_config_path() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir.join("cast-interop").join("config.toml");
    }
    PathBuf::from("./config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a Config by parsing a TOML string (path left empty).
    fn config_from_toml(toml: &str) -> Config {
        let mut cfg: Config = toml::from_str(toml).expect("valid toml");
        cfg.path = PathBuf::new();
        cfg
    }

    // -------------------------------------------------------------------------
    // TOML deserialization – [rpc] legacy section
    // -------------------------------------------------------------------------

    #[test]
    fn toml_legacy_rpc_parses_all_fields() {
        let cfg = config_from_toml(
            r#"
[rpc]
default = "http://localhost:8545"
a = "http://chain-a:8545"
b = "http://chain-b:8545"
"#,
        );
        let rpc = cfg.rpc.as_ref().expect("rpc section present");
        assert_eq!(rpc.default.as_deref(), Some("http://localhost:8545"));
        assert_eq!(rpc.a.as_deref(), Some("http://chain-a:8545"));
        assert_eq!(rpc.b.as_deref(), Some("http://chain-b:8545"));
        assert!(cfg.chains.is_none());
    }

    #[test]
    fn toml_legacy_rpc_partial_fields() {
        let cfg = config_from_toml(
            r#"
[rpc]
default = "http://localhost:8545"
"#,
        );
        let rpc = cfg.rpc.as_ref().expect("rpc section present");
        assert_eq!(rpc.default.as_deref(), Some("http://localhost:8545"));
        assert!(rpc.a.is_none());
        assert!(rpc.b.is_none());
    }

    // -------------------------------------------------------------------------
    // TOML deserialization – [chains.<alias>] new section
    // -------------------------------------------------------------------------

    #[test]
    fn toml_chains_section_with_chain_id() {
        let cfg = config_from_toml(
            r#"
[chains.mainnet]
rpc = "http://mainnet:8545"
chainId = 1

[chains.testnet]
rpc = "http://testnet:8545"
chainId = 11155111
"#,
        );
        let chains = cfg.chains.as_ref().expect("chains section present");
        assert_eq!(chains.len(), 2);

        let mainnet = chains.get("mainnet").expect("mainnet present");
        assert_eq!(mainnet.rpc, "http://mainnet:8545");
        assert_eq!(mainnet.chain_id, Some(1));

        let testnet = chains.get("testnet").expect("testnet present");
        assert_eq!(testnet.rpc, "http://testnet:8545");
        assert_eq!(testnet.chain_id, Some(11155111));
    }

    #[test]
    fn toml_chain_without_chain_id_is_valid() {
        let cfg = config_from_toml(
            r#"
[chains.local]
rpc = "http://localhost:8545"
"#,
        );
        let chains = cfg.chains.as_ref().expect("chains present");
        let local = chains.get("local").expect("local present");
        assert_eq!(local.rpc, "http://localhost:8545");
        assert!(local.chain_id.is_none());
    }

    // -------------------------------------------------------------------------
    // TOML deserialization – [addresses]
    // -------------------------------------------------------------------------

    #[test]
    fn toml_addresses_section_parses_correctly() {
        let cfg = config_from_toml(
            r#"
[addresses]
interop_center = "0xAAAA000000000000000000000000000000000001"
interop_handler = "0xBBBB000000000000000000000000000000000002"
interop_root_storage = "0xCCCC000000000000000000000000000000000003"
"#,
        );
        let addr = cfg.addresses.as_ref().expect("addresses present");
        assert_eq!(
            addr.interop_center.as_deref(),
            Some("0xAAAA000000000000000000000000000000000001")
        );
        assert_eq!(
            addr.interop_handler.as_deref(),
            Some("0xBBBB000000000000000000000000000000000002")
        );
        assert_eq!(
            addr.interop_root_storage.as_deref(),
            Some("0xCCCC000000000000000000000000000000000003")
        );
    }

    // -------------------------------------------------------------------------
    // TOML deserialization – [abi] and [signer]
    // -------------------------------------------------------------------------

    #[test]
    fn toml_abi_dir_parses() {
        let cfg = config_from_toml(
            r#"
[abi]
dir = "/opt/abis"
"#,
        );
        assert_eq!(cfg.abi_dir(), PathBuf::from("/opt/abis"));
    }

    #[test]
    fn toml_signer_env_parses() {
        let cfg = config_from_toml(
            r#"
[signer]
private_key_env = "MY_KEY"
"#,
        );
        assert_eq!(cfg.signer_env(), "MY_KEY");
    }

    // -------------------------------------------------------------------------
    // Config defaults
    // -------------------------------------------------------------------------

    #[test]
    fn default_config_has_no_sections() {
        let cfg = Config::default();
        assert!(cfg.rpc.is_none());
        assert!(cfg.chains.is_none());
        assert!(cfg.addresses.is_none());
        assert!(cfg.abi.is_none());
        assert!(cfg.signer.is_none());
    }

    #[test]
    fn abi_dir_fallback_when_not_configured() {
        let cfg = Config::default();
        assert_eq!(cfg.abi_dir(), PathBuf::from("./deps"));
    }

    #[test]
    fn signer_env_fallback_when_not_configured() {
        let cfg = Config::default();
        assert_eq!(cfg.signer_env(), "PRIVATE_KEY");
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – explicit --rpc wins
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_explicit_url_overrides_everything() {
        let cfg = config_from_toml(
            r#"
[chains.default]
rpc = "http://default:8545"
chainId = 1
"#,
        );
        let resolved = cfg
            .resolve_rpc(Some("http://explicit:9999"), None)
            .expect("should succeed");
        assert_eq!(resolved.url, "http://explicit:9999");
        assert!(resolved.alias.is_none());
        assert!(resolved.chain_id.is_none());
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – both --rpc and --chain set is an error
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_both_rpc_and_chain_is_error() {
        let cfg = Config::default();
        let err = cfg
            .resolve_rpc(Some("http://host:8545"), Some("mainnet"))
            .unwrap_err();
        assert!(
            err.to_string().contains("cannot set both"),
            "unexpected error: {err}"
        );
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – chain alias lookup in [chains]
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_chain_alias_from_chains_section() {
        let cfg = config_from_toml(
            r#"
[chains.mynet]
rpc = "http://mynet:8545"
chainId = 42
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("mynet"))
            .expect("should resolve");
        assert_eq!(resolved.url, "http://mynet:8545");
        assert_eq!(resolved.alias.as_deref(), Some("mynet"));
        assert_eq!(resolved.chain_id, Some(42));
    }

    #[test]
    fn resolve_rpc_chain_alias_without_chain_id() {
        let cfg = config_from_toml(
            r#"
[chains.nochain]
rpc = "http://nochain:8545"
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("nochain"))
            .expect("should resolve");
        assert_eq!(resolved.url, "http://nochain:8545");
        assert!(resolved.chain_id.is_none());
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – legacy [rpc] alias lookup
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_legacy_alias_a() {
        let cfg = config_from_toml(
            r#"
[rpc]
a = "http://chain-a:8545"
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("a"))
            .expect("should resolve legacy alias a");
        assert_eq!(resolved.url, "http://chain-a:8545");
        assert_eq!(resolved.alias.as_deref(), Some("a"));
        assert!(resolved.chain_id.is_none());
    }

    #[test]
    fn resolve_rpc_legacy_alias_b() {
        let cfg = config_from_toml(
            r#"
[rpc]
b = "http://chain-b:8545"
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("b"))
            .expect("should resolve legacy alias b");
        assert_eq!(resolved.url, "http://chain-b:8545");
        assert_eq!(resolved.alias.as_deref(), Some("b"));
    }

    #[test]
    fn resolve_rpc_legacy_alias_default() {
        let cfg = config_from_toml(
            r#"
[rpc]
default = "http://legacy-default:8545"
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("default"))
            .expect("should resolve legacy default alias");
        assert_eq!(resolved.url, "http://legacy-default:8545");
        assert_eq!(resolved.alias.as_deref(), Some("default"));
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – unknown alias is an error
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_unknown_chain_alias_is_error() {
        let cfg = config_from_toml(
            r#"
[chains.known]
rpc = "http://known:8545"
"#,
        );
        let err = cfg.resolve_rpc(None, Some("unknown")).unwrap_err();
        assert!(
            err.to_string().contains("unknown chain alias"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_rpc_no_config_and_unknown_alias_is_error() {
        let cfg = Config::default();
        let err = cfg.resolve_rpc(None, Some("ghost")).unwrap_err();
        assert!(
            err.to_string().contains("unknown chain alias"),
            "unexpected error: {err}"
        );
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – auto-resolution with no alias specified
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_no_args_uses_chains_default() {
        let cfg = config_from_toml(
            r#"
[chains.default]
rpc = "http://default-chain:8545"
chainId = 7

[chains.other]
rpc = "http://other:8545"
chainId = 8
"#,
        );
        let resolved = cfg.resolve_rpc(None, None).expect("should pick default");
        assert_eq!(resolved.url, "http://default-chain:8545");
        assert_eq!(resolved.alias.as_deref(), Some("default"));
        assert_eq!(resolved.chain_id, Some(7));
    }

    #[test]
    fn resolve_rpc_no_args_single_chain_is_used() {
        let cfg = config_from_toml(
            r#"
[chains.only]
rpc = "http://only:8545"
chainId = 99
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, None)
            .expect("single chain should be auto-resolved");
        assert_eq!(resolved.url, "http://only:8545");
        assert_eq!(resolved.chain_id, Some(99));
    }

    #[test]
    fn resolve_rpc_no_args_falls_back_to_legacy_default() {
        let cfg = config_from_toml(
            r#"
[rpc]
default = "http://legacy:8545"
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, None)
            .expect("legacy default should be used");
        assert_eq!(resolved.url, "http://legacy:8545");
        assert_eq!(resolved.alias.as_deref(), Some("default"));
    }

    #[test]
    fn resolve_rpc_no_config_no_args_is_error() {
        let cfg = Config::default();
        let err = cfg.resolve_rpc(None, None).unwrap_err();
        assert!(
            err.to_string().contains("no rpc configured"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_rpc_multiple_chains_no_default_is_error() {
        // More than one chain, none named "default" — auto-resolution must fail.
        let cfg = config_from_toml(
            r#"
[chains.alpha]
rpc = "http://alpha:8545"

[chains.beta]
rpc = "http://beta:8545"
"#,
        );
        let err = cfg.resolve_rpc(None, None).unwrap_err();
        assert!(
            err.to_string().contains("no rpc configured"),
            "unexpected error: {err}"
        );
    }

    // -------------------------------------------------------------------------
    // resolve_rpc – [chains] takes priority over legacy [rpc] for same alias
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_rpc_chains_section_takes_priority_over_legacy_for_alias() {
        // Both [chains.a] and [rpc].a are present; chains wins.
        let cfg = config_from_toml(
            r#"
[rpc]
a = "http://legacy-a:8545"

[chains.a]
rpc = "http://new-a:8545"
chainId = 5
"#,
        );
        let resolved = cfg
            .resolve_rpc(None, Some("a"))
            .expect("should resolve via chains section");
        assert_eq!(resolved.url, "http://new-a:8545");
        assert_eq!(resolved.chain_id, Some(5));
    }

    // -------------------------------------------------------------------------
    // chain() / set_chain() / remove_chain()
    // -------------------------------------------------------------------------

    #[test]
    fn set_chain_inserts_and_chain_retrieves() {
        let mut cfg = Config::default();
        cfg.set_chain("dev".to_string(), "http://dev:8545".to_string(), 1337);
        let c = cfg.chain("dev").expect("dev should be present");
        assert_eq!(c.rpc, "http://dev:8545");
        assert_eq!(c.chain_id, Some(1337));
    }

    #[test]
    fn set_chain_overwrites_existing_entry() {
        let mut cfg = Config::default();
        cfg.set_chain("dev".to_string(), "http://old:8545".to_string(), 1);
        cfg.set_chain("dev".to_string(), "http://new:8545".to_string(), 2);
        let c = cfg.chain("dev").expect("dev should be present");
        assert_eq!(c.rpc, "http://new:8545");
        assert_eq!(c.chain_id, Some(2));
    }

    #[test]
    fn remove_chain_returns_true_when_alias_exists() {
        let mut cfg = Config::default();
        cfg.set_chain("temp".to_string(), "http://temp:8545".to_string(), 9);
        assert!(cfg.remove_chain("temp"));
        assert!(cfg.chain("temp").is_none());
    }

    #[test]
    fn remove_chain_returns_false_when_alias_missing() {
        let mut cfg = Config::default();
        assert!(!cfg.remove_chain("nonexistent"));
    }

    #[test]
    fn chain_returns_none_for_unknown_alias() {
        let cfg = config_from_toml(
            r#"
[chains.known]
rpc = "http://known:8545"
"#,
        );
        assert!(cfg.chain("unknown").is_none());
    }

    // -------------------------------------------------------------------------
    // resolve_chain_id
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_chain_id_from_alias_with_chain_id() {
        let cfg = config_from_toml(
            r#"
[chains.zk]
rpc = "http://zk:8545"
chainId = 324
"#,
        );
        let id = cfg.resolve_chain_id("zk").expect("should resolve");
        assert_eq!(id, alloy_primitives::U256::from(324u64));
    }

    #[test]
    fn resolve_chain_id_alias_missing_chain_id_is_error() {
        let cfg = config_from_toml(
            r#"
[chains.noid]
rpc = "http://noid:8545"
"#,
        );
        let err = cfg.resolve_chain_id("noid").unwrap_err();
        assert!(
            err.to_string().contains("chainId missing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_chain_id_numeric_string_parsed_directly() {
        let cfg = Config::default();
        let id = cfg.resolve_chain_id("42161").expect("should parse as number");
        assert_eq!(id, alloy_primitives::U256::from(42161u64));
    }

    #[test]
    fn resolve_chain_id_invalid_string_is_error() {
        let cfg = Config::default();
        let err = cfg.resolve_chain_id("not-a-number").unwrap_err();
        assert!(
            err.to_string().contains("invalid"),
            "unexpected error: {err}"
        );
    }

    // -------------------------------------------------------------------------
    // Config::load – file on disk
    // -------------------------------------------------------------------------

    #[test]
    fn load_nonexistent_path_returns_default_config() {
        let tmp = std::env::temp_dir().join("cast_interop_test_nonexistent_xyz.toml");
        // Make sure file does not exist.
        let _ = std::fs::remove_file(&tmp);
        let cfg = Config::load(Some(&tmp)).expect("load should succeed");
        assert!(cfg.rpc.is_none());
        assert!(cfg.chains.is_none());
        assert_eq!(cfg.path, tmp);
    }

    #[test]
    fn load_valid_toml_file_populates_config() {
        let tmp = std::env::temp_dir().join("cast_interop_test_load_valid.toml");
        std::fs::write(
            &tmp,
            r#"
[chains.local]
rpc = "http://localhost:8545"
chainId = 270
"#,
        )
        .expect("write test file");

        let cfg = Config::load(Some(&tmp)).expect("load should succeed");
        let chains = cfg.chains.as_ref().expect("chains present");
        let local = chains.get("local").expect("local present");
        assert_eq!(local.rpc, "http://localhost:8545");
        assert_eq!(local.chain_id, Some(270));
        assert_eq!(cfg.path, tmp);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let tmp = std::env::temp_dir().join("cast_interop_test_load_invalid.toml");
        std::fs::write(&tmp, "[[[ not valid toml").expect("write test file");

        let err = Config::load(Some(&tmp)).unwrap_err();
        assert!(
            err.to_string().contains("failed to parse config"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_file(&tmp);
    }
}
