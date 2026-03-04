use crate::types::format_hex;
use alloy_primitives::Address;
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

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

/// Extract the RFC 3986 authority (host[:port]) from a URL for use as the SIWE domain.
fn siwe_domain(api_base_url: &str) -> String {
    url::Url::parse(api_base_url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            })
        })
        .unwrap_or_else(|| api_base_url.to_string())
}

async fn request_siwe_message(http: &Client, api_base_url: &str, address: Address) -> Result<String> {
    let url = format!("{}/api/siwe-messages", api_base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "address": format!("{address:#x}"),
        "domain": siwe_domain(api_base_url),
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
    Ok(format_hex(&signature.as_bytes()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_key_takes_priority() {
        let key = "0xdeadbeef";
        let result = resolve_private_key(Some(key), Some("SOME_ENV_VAR")).unwrap();
        assert_eq!(result, key);
    }

    #[test]
    fn resolve_key_from_named_env_var() {
        std::env::set_var("TEST_PRIV_KEY_ABC123", "0x1234abcd");
        let result = resolve_private_key(None, Some("TEST_PRIV_KEY_ABC123")).unwrap();
        assert_eq!(result, "0x1234abcd");
        std::env::remove_var("TEST_PRIV_KEY_ABC123");
    }

    #[test]
    fn resolve_key_from_default_env_var() {
        // Temporarily set the default env var
        std::env::set_var("PRIVIDIUM_PRIVATE_KEY", "0xabcdef");
        let result = resolve_private_key(None, None).unwrap();
        assert_eq!(result, "0xabcdef");
        std::env::remove_var("PRIVIDIUM_PRIVATE_KEY");
    }

    #[test]
    fn missing_env_var_returns_error() {
        std::env::remove_var("PRIVIDIUM_PRIVATE_KEY_MISSING_XYZ");
        let err = resolve_private_key(None, Some("PRIVIDIUM_PRIVATE_KEY_MISSING_XYZ"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("PRIVIDIUM_PRIVATE_KEY_MISSING_XYZ"), "got: {err}");
        assert!(err.contains("not set"), "got: {err}");
    }

    #[test]
    fn missing_default_env_var_mentions_prividium_private_key() {
        std::env::remove_var("PRIVIDIUM_PRIVATE_KEY");
        let err = resolve_private_key(None, None).unwrap_err().to_string();
        assert!(err.contains("PRIVIDIUM_PRIVATE_KEY"), "got: {err}");
    }

    #[tokio::test]
    async fn sign_message_produces_0x_prefixed_hex() {
        // Known test private key (not used for anything real)
        let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let signer: PrivateKeySigner = private_key.parse().unwrap();
        let result = sign_message(&signer, "hello world").await.unwrap();
        assert!(result.starts_with("0x"), "signature should be 0x-prefixed, got: {result}");
        // EIP-191 signature is 65 bytes = 130 hex chars + "0x" prefix
        assert_eq!(result.len(), 132, "signature should be 132 chars (0x + 130 hex), got: {result}");
    }

    #[test]
    fn invalid_private_key_returns_parse_error() {
        // authenticate() itself does network I/O, but we can test key parsing
        let bad_key = "not_a_hex_key";
        let result: std::result::Result<PrivateKeySigner, _> = bad_key.parse();
        assert!(result.is_err());
    }

    #[test]
    fn siwe_domain_extracts_host_from_https_url() {
        assert_eq!(siwe_domain("https://permissions.example.com"), "permissions.example.com");
    }

    #[test]
    fn siwe_domain_includes_non_default_port() {
        assert_eq!(siwe_domain("https://permissions.example.com:8443/some/path"), "permissions.example.com:8443");
    }

    #[test]
    fn siwe_domain_omits_default_https_port() {
        // url::Url strips the default port (443 for https)
        assert_eq!(siwe_domain("https://permissions.example.com:443"), "permissions.example.com");
    }

    #[test]
    fn siwe_domain_falls_back_to_raw_url_on_invalid_input() {
        let raw = "not-a-url";
        assert_eq!(siwe_domain(raw), raw);
    }
}
