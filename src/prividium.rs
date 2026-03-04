use alloy_primitives::Address;
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const PRIVIDIUM_DOMAIN: &str = "localhost:3000";

/// Bearer token returned by Prividium login.
#[derive(Debug, Clone)]
pub struct SessionToken {
    pub token: String,
}

#[derive(Debug, Deserialize)]
struct SiweMessageResponse {
    msg: String,
}

#[derive(Debug, Serialize)]
struct LoginRequest {
    message: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    token: Option<String>,
    requires_mfa: Option<bool>,
}

/// Performs the 3-step SIWE auth flow against a Prividium API and returns a
/// bearer token.
///
/// Steps:
/// 1. POST /api/siwe-messages  — obtain nonce-bearing message
/// 2. Sign the message with the wallet private key (EIP-191 personal_sign)
/// 3. POST /api/auth/login/crypto-native  — submit signature, receive token
pub async fn authenticate(api_base_url: &str, private_key: &str) -> Result<SessionToken> {
    let signer: PrivateKeySigner = private_key
        .parse()
        .map_err(|e| anyhow!("invalid prividium private key: {e}"))?;

    let address = signer.address();
    let http = Client::new();

    let msg = request_siwe_message(&http, api_base_url, address).await?;
    let signature = sign_message(&signer, &msg).await?;
    submit_signature(&http, api_base_url, &msg, &signature).await
}

async fn request_siwe_message(http: &Client, api_base_url: &str, address: Address) -> Result<String> {
    let url = format!("{}/api/siwe-messages", api_base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "address": format!("{address:#x}"),
        "domain": PRIVIDIUM_DOMAIN,
    });

    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("failed to reach Prividium API for SIWE message")?;

    let status = resp.status();
    if status.as_u16() == 404 {
        anyhow::bail!(
            "wallet address {address:#x} not found in Prividium — ensure it is registered"
        );
    }
    if status.as_u16() == 429 {
        anyhow::bail!("Prividium rate limit reached — try again in a few minutes");
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Prividium SIWE message request failed ({status}): {body}");
    }

    let parsed: SiweMessageResponse = resp
        .json()
        .await
        .context("failed to parse Prividium SIWE message response")?;

    Ok(parsed.msg)
}

async fn sign_message(signer: &PrivateKeySigner, message: &str) -> Result<String> {
    let signature = signer
        .sign_message(message.as_bytes())
        .await
        .context("failed to sign SIWE message")?;
    Ok(format!("0x{}", hex::encode(signature.as_bytes())))
}

async fn submit_signature(
    http: &Client,
    api_base_url: &str,
    message: &str,
    signature: &str,
) -> Result<SessionToken> {
    let url = format!(
        "{}/api/auth/login/crypto-native",
        api_base_url.trim_end_matches('/')
    );
    let body = LoginRequest {
        message: message.to_string(),
        signature: signature.to_string(),
    };

    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("failed to reach Prividium login endpoint")?;

    let status = resp.status();
    if status.as_u16() == 403 {
        anyhow::bail!("Prividium login failed — signature rejected or nonce already used");
    }
    if status.as_u16() == 429 {
        anyhow::bail!("Prividium rate limit reached on login endpoint");
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Prividium login failed ({status}): {body}");
    }

    let parsed: LoginResponse = resp
        .json()
        .await
        .context("failed to parse Prividium login response")?;

    if parsed.requires_mfa == Some(true) {
        anyhow::bail!(
            "Prividium login requires MFA (passkey) which is not supported for programmatic auth — \
             use a non-admin wallet address"
        );
    }

    let token = parsed
        .token
        .ok_or_else(|| anyhow!("Prividium login response missing token field"))?;

    Ok(SessionToken { token })
}

/// Resolve the private key for Prividium auth.
/// Priority: explicit value → env var name → "PRIVIDIUM_PRIVATE_KEY".
pub fn resolve_private_key(explicit: Option<&str>, env_name: Option<&str>) -> Result<String> {
    if let Some(key) = explicit {
        return Ok(key.to_string());
    }
    let env = env_name.unwrap_or("PRIVIDIUM_PRIVATE_KEY");
    std::env::var(env).map_err(|_| {
        anyhow!("Prividium private key not set — set the {env} environment variable")
    })
}
