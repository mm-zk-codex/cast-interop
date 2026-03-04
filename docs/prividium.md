# Prividium Authentication

[Prividium](https://github.com/mm-zk-codex) chains gate RPC access behind SIWE (Sign-In with Ethereum) authentication. `cast-interop` implements this flow automatically — once a chain alias is registered, every command authenticates transparently before making RPC requests.

## Prerequisites

* A wallet address registered in the Prividium system
* The corresponding private key
* The Prividium API base URL (e.g. `https://permissions.example.com`)

## Setup

### 1. Register the chain alias

```bash
cast-interop chains add mychain \
  --rpc https://permissions.example.com/rpc \
  --prividium-url https://permissions.example.com \
  --prividium-key-env PRIVIDIUM_PRIVATE_KEY
```

`--prividium-url` is the Prividium permissions API base URL (not the RPC URL). The CLI will authenticate against this URL before sending RPC requests to `--rpc`.

`--prividium-key-env` is the name of the environment variable holding the private key. Defaults to `PRIVIDIUM_PRIVATE_KEY`.

### 2. Set the private key

```bash
export PRIVIDIUM_PRIVATE_KEY=0xYourPrivateKeyHex
```

### 3. Use normally

```bash
cast-interop bundle relay \
  --chain-src mychain \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --private-key $PRIVATE_KEY

cast-interop debug tx --chain mychain 0xTX_HASH

cast-interop token send \
  --chain-src mychain \
  --chain-dest test \
  --token 0xTOKEN \
  --amount 1 \
  --to 0xRECIPIENT \
  --private-key $PRIVATE_KEY
```

## Manual config (config.toml)

```toml
[chains.mychain]
rpc = "https://permissions.example.com/rpc"
chainId = 270
prividium_url = "https://permissions.example.com"
prividium_key_env = "PRIVIDIUM_PRIVATE_KEY"
```

`prividium_key_env` can be omitted — it defaults to `PRIVIDIUM_PRIVATE_KEY`.

## Authentication flow

Before the first RPC call on a Prividium chain, the CLI performs:

1. `POST {prividium_url}/api/siwe-messages` — obtain a nonce-bearing SIWE message
2. Sign the message with the configured private key (EIP-191 `personal_sign`)
3. `POST {prividium_url}/api/auth/login/crypto-native` — submit signature, receive bearer token
4. All subsequent RPC requests include `Authorization: Bearer <token>`

Session tokens are valid for several hours. A new token is obtained per CLI invocation.

## Errors

| Error | Cause |
|---|---|
| `wallet address not found in Prividium` | The address is not registered in the Prividium system |
| `Prividium login requires MFA` | Admin account with passkey — use a non-admin wallet |
| `Prividium private key not set` | The configured env var is not set |
| `Prividium rate limit` | Too many auth attempts — wait a few minutes |
| `Prividium login failed — signature rejected` | Nonce reuse or clock skew — retry |
