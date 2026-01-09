use crate::config::Config;
use alloy_primitives::Address;
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use anyhow::{anyhow, Result};

pub struct SignerOptions<'a> {
    pub private_key: Option<&'a str>,
    pub private_key_env: Option<&'a str>,
}

pub fn load_signer(options: SignerOptions<'_>, config: &Config) -> Result<Option<PrivateKeySigner>> {
    if options.private_key.is_some() && options.private_key_env.is_some() {
        anyhow::bail!("cannot set both --private-key and --private-key-env");
    }

    let env = options
        .private_key_env
        .map(|value| value.to_string())
        .unwrap_or_else(|| config.signer_env());

    if let Some(key) = options.private_key {
        return Ok(Some(load_wallet(key)?));
    }
    if let Ok(key) = std::env::var(env) {
        return Ok(Some(load_wallet(&key)?));
    }
    Ok(None)
}

pub fn signer_address(signer: &PrivateKeySigner) -> Result<Address> {
    Ok(signer.address())
}

fn load_wallet(key: &str) -> Result<PrivateKeySigner> {
    let pk_signer: PrivateKeySigner = key
        .parse()
        .map_err(|err| anyhow!("invalid private key: {err}"))?;
    Ok(pk_signer)
}
