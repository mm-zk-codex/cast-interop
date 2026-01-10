use crate::abi::{
    decode_interop_bundle_sent, encode_bundle_status_call, encode_execute_bundle_call,
    encode_interop_bundle, encode_send_bundle_call, encode_verify_bundle_call,
    interop_bundle_sent_topic,
};
use crate::cli::{TokenBalanceArgs, TokenInfoArgs, TokenSendArgs};
use crate::commands::bundle_action::decode_send_transaction;
use crate::config::{Config, ResolvedRpc};
use crate::encode::{
    encode_asset_id, encode_evm_v1_address_only, encode_evm_v1_chain_only, encode_indirect_call,
    encode_interop_call_value, encode_unbundler_address, DEFAULT_NATIVE_TOKEN_VAULT,
};
use crate::rpc::{
    eth_call, eth_call_with_value, get_transaction_receipt, wait_for_finalized_block,
    wait_for_log_proof, RpcClient,
};
use crate::signer::{load_signer, SignerOptions};
use crate::types::{
    address_to_hex, format_hex, parse_address, parse_u256, require_signer_or_dry_run, AddressBook,
    MessageInclusionProof, ProofMessage, BUNDLE_IDENTIFIER, DEFAULT_ASSET_ROUTER,
};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::{TransactionInput, TransactionRequest};
use alloy_sol_types::{SolCall, SolValue};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::str::FromStr;
use std::time::Duration;

alloy_sol_types::sol! {
    function balanceOf(address account) view returns (uint256);
    function approve(address spender, uint256 value) returns (bool);
    function decimals() view returns (uint8);
    function symbol() view returns (string);
    function name() view returns (string);

    function ensureTokenIsRegistered(address _token) returns (bytes32);
    function tokenAddress(bytes32 _assetId) view returns (address);
}

const NEW_ENCODING_VERSION: u8 = 0x01;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenInfoOutput {
    src_chain_id: String,
    dest_chain_id: String,
    token_on_src: String,
    native_token_vault: String,
    asset_id: String,
    wrapped_token_on_dest: String,
    symbol: Option<String>,
    name: Option<String>,
    decimals: Option<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenBalanceOutput {
    src_chain_id: String,
    dest_chain_id: String,
    token_on_src: String,
    native_token_vault: String,
    asset_id: String,
    wrapped_token_on_dest: String,
    balance: Option<String>,
    balance_raw: Option<String>,
    decimals: Option<u8>,
}

/// Resolve wrapped token metadata on the destination chain.
///
/// Returns the asset ID plus optional symbol/name/decimals if the wrapped
/// token has been deployed.
pub async fn run_info(args: TokenInfoArgs, config: Config, _addresses: AddressBook) -> Result<()> {
    let src_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;
    let src_client = RpcClient::new(&src_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let src_chain_id = src_client.provider.get_chain_id().await?;
    let dest_chain_id = dest_client.provider.get_chain_id().await?;

    let token = parse_address(&args.token)?;
    let vault = parse_address(
        args.native_token_vault
            .as_deref()
            .unwrap_or(DEFAULT_NATIVE_TOKEN_VAULT),
    )?;

    let asset_id = encode_asset_id(U256::from(src_chain_id), token, vault);
    let asset_id_hex = format_hex(asset_id.as_ref());
    let wrapped_token = fetch_wrapped_token(&dest_client, vault, &asset_id).await?;

    let (symbol, name, decimals) = if wrapped_token != Address::ZERO {
        let symbol = fetch_symbol(&dest_client, wrapped_token).await;
        let name = fetch_name(&dest_client, wrapped_token).await;
        let decimals = fetch_decimals(&dest_client, wrapped_token)
            .await
            .and_then(|value| u8::try_from(value).ok());
        (symbol, name, decimals)
    } else {
        (None, None, None)
    };

    let output = TokenInfoOutput {
        src_chain_id: src_chain_id.to_string(),
        dest_chain_id: dest_chain_id.to_string(),
        token_on_src: address_to_hex(token),
        native_token_vault: address_to_hex(vault),
        asset_id: asset_id_hex,
        wrapped_token_on_dest: address_to_hex(wrapped_token),
        symbol,
        name,
        decimals,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("source chainId: {}", output.src_chain_id);
    println!("destination chainId: {}", output.dest_chain_id);
    println!("token (source): {}", output.token_on_src);
    println!("native token vault: {}", output.native_token_vault);
    println!("assetId: {}", output.asset_id);
    println!("wrapped token (dest): {}", output.wrapped_token_on_dest);
    if let Some(symbol) = output.symbol.as_deref() {
        println!("symbol: {symbol}");
    }
    if let Some(name) = output.name.as_deref() {
        println!("name: {name}");
    }
    if let Some(decimals) = output.decimals {
        println!("decimals: {decimals}");
    }

    Ok(())
}

/// Fetch the wrapped token balance for a destination recipient.
///
/// This command also reports the wrapped token address and decimals when
/// available.
pub async fn run_balance(
    args: TokenBalanceArgs,
    config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let src_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;
    let src_client = RpcClient::new(&src_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let src_chain_id = src_client.provider.get_chain_id().await?;
    let dest_chain_id = dest_client.provider.get_chain_id().await?;

    let token = parse_address(&args.token)?;
    let to = parse_address(&args.to)?;
    let vault = parse_address(
        args.native_token_vault
            .as_deref()
            .unwrap_or(DEFAULT_NATIVE_TOKEN_VAULT),
    )?;

    let asset_id = encode_asset_id(U256::from(src_chain_id), token, vault);
    let asset_id_hex = format_hex(asset_id.as_ref());
    let wrapped_token = fetch_wrapped_token(&dest_client, vault, &asset_id).await?;

    let (balance, balance_raw, decimals) = if wrapped_token == Address::ZERO {
        (None, None, None)
    } else {
        let balance = fetch_balance(&dest_client, wrapped_token, to).await?;
        let decimals = fetch_decimals(&dest_client, wrapped_token)
            .await
            .and_then(|value| u8::try_from(value).ok());
        let balance_raw = Some(balance.to_string());
        let formatted = decimals
            .map(|value| format_units(balance, value as u32))
            .unwrap_or_else(|| balance.to_string());
        (Some(formatted), balance_raw, decimals)
    };

    let output = TokenBalanceOutput {
        src_chain_id: src_chain_id.to_string(),
        dest_chain_id: dest_chain_id.to_string(),
        token_on_src: address_to_hex(token),
        native_token_vault: address_to_hex(vault),
        asset_id: asset_id_hex,
        wrapped_token_on_dest: address_to_hex(wrapped_token),
        balance,
        balance_raw,
        decimals,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("source chainId: {}", output.src_chain_id);
    println!("destination chainId: {}", output.dest_chain_id);
    println!("token (source): {}", output.token_on_src);
    println!("native token vault: {}", output.native_token_vault);
    println!("assetId: {}", output.asset_id);
    println!("wrapped token (dest): {}", output.wrapped_token_on_dest);
    if wrapped_token == Address::ZERO {
        println!("Wrapped token not registered on destination yet");
        return Ok(());
    }

    if let Some(balance) = output.balance.as_deref() {
        println!("balance: {balance}");
    }
    if let Some(balance_raw) = output.balance_raw.as_deref() {
        println!("balance (raw): {balance_raw}");
    }
    if let Some(decimals) = output.decimals {
        println!("decimals: {decimals}");
    }

    Ok(())
}

/// Send an ERC20 across chains via the interop asset router.
///
/// The flow registers the token, approves allowance, sends the bundle, and can
/// optionally watch for proof/root propagation.
pub async fn run_send(args: TokenSendArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let src_rpc = config.resolve_rpc(args.rpc_src.as_deref(), args.chain_src.as_deref())?;
    let dest_rpc = config.resolve_rpc(args.rpc_dest.as_deref(), args.chain_dest.as_deref())?;

    let source_client = RpcClient::new(&src_rpc.url).await?;
    let dest_client = RpcClient::new(&dest_rpc.url).await?;

    let src_chain_id = source_client.provider.get_chain_id().await?;
    let dest_chain_id = dest_client.provider.get_chain_id().await?;

    let token = parse_address(&args.token)?;
    let to = parse_address(&args.to)?;
    let vault = parse_address(
        args.native_token_vault
            .as_deref()
            .unwrap_or(DEFAULT_NATIVE_TOKEN_VAULT),
    )?;
    let asset_router = parse_address(args.asset_router.as_deref().unwrap_or(DEFAULT_ASSET_ROUTER))?;
    let unbundler = parse_address(args.unbundler.as_deref().unwrap_or(&args.to))?;

    let wallet = load_signer(
        SignerOptions {
            private_key: args.signer.private_key.as_deref(),
            private_key_env: args.signer.private_key_env.as_deref(),
        },
        &config,
    )?;

    require_signer_or_dry_run(wallet.is_some(), args.dry_run, "token send")?;

    let asset_id = encode_asset_id(U256::from(src_chain_id), token, vault);
    let asset_id_hex = format_hex(asset_id.as_ref());

    let decimals = match args.decimals {
        Some(value) => Some(value),
        None => fetch_decimals(&source_client, token).await,
    };

    let amount_wei = resolve_amount_wei(&args, decimals).await?;

    println!("=== token send preflight ===");
    println!(
        "source: {} (chainId {})",
        format_rpc(&src_rpc),
        src_chain_id
    );
    println!(
        "destination: {} (chainId {})",
        format_rpc(&dest_rpc),
        dest_chain_id
    );
    println!("token (source): {}", address_to_hex(token));
    println!("recipient (dest): {}", address_to_hex(to));
    println!("assetId: {asset_id_hex}");
    println!("asset router: {}", address_to_hex(asset_router));
    println!("native token vault: {}", address_to_hex(vault));
    println!(
        "interop center: {}",
        address_to_hex(addresses.interop_center)
    );
    println!(
        "interop handler: {}",
        address_to_hex(addresses.interop_handler)
    );
    println!(
        "interop root storage: {}",
        address_to_hex(addresses.interop_root_storage)
    );
    println!("amount (wei): {amount_wei}");
    if let Some(decimals) = decimals {
        println!("amount (formatted): {}", format_units(amount_wei, decimals));
    }
    if args.watch {
        println!("watch: enabled");
    }

    let dest_chain_id_u256 = U256::from(dest_chain_id);

    if !args.skip_register {
        let call = ensureTokenIsRegisteredCall { _token: token };
        let data = Bytes::from(call.abi_encode());
        if args.dry_run {
            let _ = eth_call(&source_client, vault, data).await;
            println!("registerTx: dry-run (eth_call)");
        } else {
            let tx_hash =
                send_tx(&source_client, &src_rpc, wallet.as_ref(), vault, data, None).await?;
            println!("registerTx: {tx_hash}");
            print_tx_debug("register", &src_rpc, &tx_hash);
        }
    }

    if !args.skip_approve {
        let approve_amount = resolve_approve_amount(&args, amount_wei)?;
        let call = approveCall {
            spender: vault,
            value: approve_amount,
        };
        let data = Bytes::from(call.abi_encode());
        if args.dry_run {
            let _ = eth_call(&source_client, token, data).await;
            println!("approveTx: dry-run (eth_call)");
        } else {
            let tx_hash =
                send_tx(&source_client, &src_rpc, wallet.as_ref(), token, data, None).await?;
            println!("approveTx: {tx_hash}");
            print_tx_debug("approve", &src_rpc, &tx_hash);
        }
    }

    let indirect_msg_value = parse_u256(&args.indirect_msg_value)?;
    let mut call_attributes = vec![encode_indirect_call(indirect_msg_value)];
    let mut total_value = indirect_msg_value;
    if let Some(interop_value) = args.interop_value.as_deref() {
        let parsed = parse_u256(interop_value)?;
        total_value += parsed;
        call_attributes.push(encode_interop_call_value(parsed));
    }

    let call_data = build_second_bridge_calldata(&asset_id, amount_wei, to, Address::ZERO)?;
    let call_starter = crate::abi::InteropCallStarter {
        to: encode_evm_v1_address_only(asset_router),
        data: call_data,
        callAttributes: call_attributes,
    };

    let bundle_attributes = vec![encode_unbundler_address(encode_evm_v1_address_only(
        unbundler,
    ))];

    let destination_chain = encode_evm_v1_chain_only(dest_chain_id_u256);
    let calldata =
        encode_send_bundle_call(destination_chain, vec![call_starter], bundle_attributes)?;

    if args.dry_run {
        let result = eth_call_with_value(
            &source_client,
            addresses.interop_center,
            calldata.clone(),
            Some(total_value),
        )
        .await?;
        let bundle_hash = crate::abi::decode_bytes32(result)?;
        println!("sendBundleTx: dry-run (eth_call)");
        println!("bundleHash: {bundle_hash:#x}");
        print_next_steps(&src_rpc, &dest_rpc, src_chain_id, "<txHash>");
        return Ok(());
    }

    let send_tx_hash = send_tx(
        &source_client,
        &src_rpc,
        wallet.as_ref(),
        addresses.interop_center,
        calldata,
        Some(total_value),
    )
    .await?;
    println!("sendBundleTx: {send_tx_hash}");
    print_tx_debug("sendBundle", &src_rpc, &send_tx_hash);

    let receipt = get_transaction_receipt(&source_client, B256::from_str(&send_tx_hash)?).await?;
    let block_number = receipt
        .block_number
        .ok_or_else(|| anyhow!("missing receipt block number"))?;
    let tx_index = receipt
        .transaction_index
        .ok_or_else(|| anyhow!("missing receipt tx index"))?;
    println!("sendBundle block: {block_number}");
    println!("sendBundle tx index: {tx_index}");

    let mut bundle = None;
    let mut bundle_hash = None;
    for log in receipt.logs().iter() {
        if log.topics().first().copied() == Some(interop_bundle_sent_topic()) {
            let (_, hash, interop_bundle) = decode_interop_bundle_sent(log.data().data.clone())?;
            bundle = Some(interop_bundle);
            bundle_hash = Some(hash);
            break;
        }
    }

    let bundle_hash = bundle_hash.ok_or_else(|| anyhow!("missing InteropBundleSent event"))?;
    println!("bundleHash: {bundle_hash:#x}");
    println!(
        "bundle status command: cast-interop bundle status {} --bundle-hash {bundle_hash:#x}",
        format_rpc_flag(&dest_rpc)
    );

    let bundle = bundle.ok_or_else(|| anyhow!("missing InteropBundleSent bundle"))?;
    let encoded_bundle = encode_interop_bundle(&bundle);

    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll = Duration::from_millis(args.poll_ms.unwrap_or(1_000));

    if args.watch {
        println!("watch: waiting for finalized block on source...");
    } else {
        println!("Waiting for finalized block on source...");
    }
    wait_for_finalized_block(
        &source_client,
        block_number,
        timeout,
        Duration::from_millis(100),
    )
    .await?;

    if args.watch {
        println!("watch: waiting for log proof on source...");
    } else {
        println!("Waiting for log proof on source...");
    }
    let log_proof = wait_for_log_proof(
        &source_client,
        B256::from_str(&send_tx_hash)?,
        0,
        timeout,
        poll,
    )
    .await?;

    println!("proof batch: {}", log_proof.batch_number);
    println!("proof msg index: {}", log_proof.id);
    println!("proof root: {}", log_proof.root);

    if args.watch {
        println!("watch: waiting for interop root on destination...");
    } else {
        println!("Waiting for interop root on destination...");
    }
    wait_for_root(
        &dest_client,
        addresses.interop_root_storage,
        src_chain_id,
        log_proof.batch_number,
        log_proof.root.clone(),
        timeout,
        poll,
    )
    .await?;

    let message = ProofMessage {
        tx_number_in_batch: tx_index,
        sender: address_to_hex(addresses.interop_center),
        data: format!(
            "0x{}{}",
            hex::encode([BUNDLE_IDENTIFIER]),
            hex::encode(encoded_bundle.as_ref())
        ),
    };

    let proof = MessageInclusionProof {
        chain_id: src_chain_id.to_string(),
        l1_batch_number: log_proof.batch_number,
        l2_message_index: log_proof.id,
        root: log_proof.root.clone(),
        message,
        proof: log_proof.proof.clone(),
    };

    let handler_calldata = match args.mode.as_str() {
        "verify" => encode_verify_bundle_call(encoded_bundle.clone(), proof.clone())?,
        "execute" => encode_execute_bundle_call(encoded_bundle.clone(), proof.clone())?,
        other => anyhow::bail!("invalid mode {other} (expected execute or verify)"),
    };

    let handler_tx_hash = send_tx(
        &dest_client,
        &dest_rpc,
        wallet.as_ref(),
        addresses.interop_handler,
        handler_calldata,
        None,
    )
    .await?;
    match args.mode.as_str() {
        "verify" => println!("verifyTx: {handler_tx_hash}"),
        _ => println!("executeTx: {handler_tx_hash}"),
    }
    print_tx_debug("handler", &dest_rpc, &handler_tx_hash);

    if args.mode == "verify" {
        let status =
            fetch_bundle_status(&dest_client, addresses.interop_handler, bundle_hash).await;
        if let Ok(status) = status {
            println!("bundle status: {}", status_string(status));
        }
        return Ok(());
    }

    let wrapped_token = fetch_wrapped_token(&dest_client, vault, &asset_id).await?;
    if wrapped_token == Address::ZERO {
        println!("wrapped token not registered on destination yet");
        return Ok(());
    }
    let balance = fetch_balance(&dest_client, wrapped_token, to).await?;
    let dest_decimals = fetch_decimals(&dest_client, wrapped_token).await;
    if let Some(decimals) = dest_decimals {
        println!("destination balance: {}", format_units(balance, decimals));
    }
    println!("destination balance (raw): {balance}");

    Ok(())
}

/// Build the calldata for the second bridge hop in a token transfer.
///
/// This is the encoded asset transfer payload used by the asset router.
fn build_second_bridge_calldata(
    asset_id: &Bytes,
    amount: U256,
    receiver: Address,
    maybe_token_address: Address,
) -> Result<Bytes> {
    let asset_id_b256 = B256::from_slice(asset_id.as_ref());
    let transfer_data = (amount, receiver, maybe_token_address).abi_encode();
    let bridge_data = (asset_id_b256, Bytes::from(transfer_data)).abi_encode_params();
    let mut out = Vec::with_capacity(1 + bridge_data.len());
    out.push(NEW_ENCODING_VERSION);
    out.extend_from_slice(&bridge_data);
    Ok(Bytes::from(out))
}

/// Resolve the approval amount based on user flags.
///
/// Accepts \"infinite\" or defaults to the send amount.
fn resolve_approve_amount(args: &TokenSendArgs, amount_wei: U256) -> Result<U256> {
    let approve_amount = match args.approve_amount.as_deref() {
        Some("infinite") => U256::MAX,
        Some(value) => parse_u256(value)?,
        None => amount_wei,
    };
    Ok(approve_amount)
}

/// Resolve the amount in wei using raw amount or decimal parsing.
///
/// Requires decimals when using human-readable amounts.
async fn resolve_amount_wei(args: &TokenSendArgs, decimals: Option<u32>) -> Result<U256> {
    if let Some(amount_wei) = args.amount_wei.as_deref() {
        return parse_u256(amount_wei);
    }
    let amount = args
        .amount
        .as_deref()
        .ok_or_else(|| anyhow!("set --amount or --amount-wei"))?;
    let decimals = decimals.ok_or_else(|| {
        anyhow!("token decimals unavailable (set --decimals or use --amount-wei)")
    })?;
    parse_decimal_amount(amount, decimals)
}

/// Parse a human-readable decimal token amount into wei.
///
/// Enforces that fractional digits do not exceed the token decimals.
fn parse_decimal_amount(amount: &str, decimals: u32) -> Result<U256> {
    let trimmed = amount.trim();
    let mut parts = trimmed.split('.');
    let whole_part = parts.next().unwrap_or("0");
    let fraction_part = parts.next();
    if parts.next().is_some() {
        anyhow::bail!("invalid amount {amount}");
    }

    let whole = if whole_part.is_empty() {
        U256::ZERO
    } else {
        parse_u256(whole_part)?
    };
    let base = pow10(decimals)?;
    let mut value = whole * base;

    if let Some(fraction_part) = fraction_part {
        if fraction_part.len() > decimals as usize {
            anyhow::bail!("amount has too many decimal places (max {decimals})");
        }
        if !fraction_part.is_empty() {
            let fraction = parse_u256(fraction_part)?;
            let scale = pow10(decimals - fraction_part.len() as u32)?;
            value += fraction * scale;
        }
    }

    Ok(value)
}

/// Compute 10^exp with overflow protection.
fn pow10(exp: u32) -> Result<U256> {
    let mut value = U256::from(1u64);
    for _ in 0..exp {
        value = value
            .checked_mul(U256::from(10u64))
            .ok_or_else(|| anyhow!("amount overflow"))?;
    }
    Ok(value)
}

/// Fetch the wrapped token address from the native token vault.
async fn fetch_wrapped_token(
    client: &RpcClient,
    vault: Address,
    asset_id: &Bytes,
) -> Result<Address> {
    let asset_id_b256 = B256::from_slice(asset_id.as_ref());
    let call = tokenAddressCall {
        _assetId: asset_id_b256,
    };
    let data = Bytes::from(call.abi_encode());
    let result = eth_call(client, vault, data).await?;
    let value: (Address,) = <(Address,)>::abi_decode(result.as_ref())?;
    Ok(value.0)
}

/// Fetch an ERC20 balance using balanceOf.
async fn fetch_balance(client: &RpcClient, token: Address, owner: Address) -> Result<U256> {
    let call = balanceOfCall { account: owner };
    let data = Bytes::from(call.abi_encode());
    let result = eth_call(client, token, data).await?;
    let value: (U256,) = <(U256,)>::abi_decode(result.as_ref())?;
    Ok(value.0)
}

/// Fetch an ERC20 decimals value, returning None if unavailable.
async fn fetch_decimals(client: &RpcClient, token: Address) -> Option<u32> {
    let call = decimalsCall {};
    let data = Bytes::from(call.abi_encode());
    let result = eth_call(client, token, data).await.ok()?;
    let value: (U256,) = <(U256,)>::abi_decode(result.as_ref()).ok()?;
    u8::try_from(value.0).ok().map(u32::from)
}

/// Fetch an ERC20 symbol, returning None if unavailable.
async fn fetch_symbol(client: &RpcClient, token: Address) -> Option<String> {
    let call = symbolCall {};
    let data = Bytes::from(call.abi_encode());
    let result = eth_call(client, token, data).await.ok()?;
    let value: (String,) = <(String,)>::abi_decode(result.as_ref()).ok()?;
    Some(value.0)
}

/// Fetch an ERC20 name, returning None if unavailable.
async fn fetch_name(client: &RpcClient, token: Address) -> Option<String> {
    let call = nameCall {};
    let data = Bytes::from(call.abi_encode());
    let result = eth_call(client, token, data).await.ok()?;
    let value: (String,) = <(String,)>::abi_decode(result.as_ref()).ok()?;
    Some(value.0)
}

/// Format a token value with the given decimals.
fn format_units(value: U256, decimals: u32) -> String {
    if decimals == 0 {
        return value.to_string();
    }
    let mut digits = value.to_string();
    if digits.len() <= decimals as usize {
        let zeros = "0".repeat(decimals as usize + 1 - digits.len());
        digits = format!("{zeros}{digits}");
    }
    let split = digits.len() - decimals as usize;
    let mut out = format!("{}.{}", &digits[..split], &digits[split..]);
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out
}

/// Send a signed transaction and wait for a receipt.
async fn send_tx(
    client: &RpcClient,
    rpc: &ResolvedRpc,
    wallet: Option<&alloy_signer_local::PrivateKeySigner>,
    to: Address,
    data: Bytes,
    value: Option<U256>,
) -> Result<String> {
    let wallet = wallet.ok_or_else(|| anyhow!("signer required"))?;
    let chain_id = client.provider.get_chain_id().await?;
    let provider = ProviderBuilder::new()
        .wallet(wallet.clone())
        .with_chain_id(chain_id)
        .connect(&rpc.url)
        .await?;

    let request = TransactionRequest {
        to: Some(to.into()),
        input: TransactionInput::new(data),
        value,
        ..Default::default()
    };

    let pending = decode_send_transaction(provider.send_transaction(request).await)?;

    let tx_hash = pending.tx_hash().clone();
    let _receipt = pending.get_receipt().await?;
    Ok(format!("{tx_hash:#x}"))
}

/// Wait for the interop root to appear on the destination chain.
async fn wait_for_root(
    client: &RpcClient,
    root_storage: Address,
    chain_id: u64,
    batch_number: u64,
    expected_root: String,
    timeout: Duration,
    poll: Duration,
) -> Result<()> {
    let expected = B256::from_str(&expected_root)?;
    let start = tokio::time::Instant::now();
    loop {
        let data =
            crate::abi::encode_interop_roots_call(U256::from(chain_id), U256::from(batch_number));
        let result = eth_call(client, root_storage, data).await?;
        let root = crate::abi::decode_bytes32(result)?;
        if root != B256::ZERO {
            if root == expected {
                return Ok(());
            }
            anyhow::bail!("interop root mismatch: expected {expected_root}, got {root:#x}");
        }
        if start.elapsed() > timeout {
            anyhow::bail!("interop root did not become available in time");
        }
        tokio::time::sleep(poll).await;
    }
}

/// Print a debug hint pointing to the decoded transaction view.
fn print_tx_debug(label: &str, rpc: &ResolvedRpc, tx_hash: &str) {
    println!("[{label}] tx: {tx_hash} ({})", format_rpc(rpc));
    println!(
        "debug: cast-interop debug tx {} {tx_hash}",
        format_rpc_flag(rpc)
    );
}

/// Print a suggested debug flow after sending a token bundle.
fn print_next_steps(
    src_rpc: &ResolvedRpc,
    dest_rpc: &ResolvedRpc,
    src_chain_id: u64,
    tx_hash: &str,
) {
    println!("Next debug steps:");
    println!(
        "  cast-interop debug tx {} {tx_hash}",
        format_rpc_flag(src_rpc)
    );
    println!(
        "  cast-interop debug proof {} --tx {tx_hash}",
        format_rpc_flag(src_rpc)
    );
    println!(
        "  cast-interop debug root {} --source-chain {} --batch <batch> --expected-root <root>",
        format_rpc_flag(dest_rpc),
        src_chain_id
    );
    println!(
        "  cast-interop bundle relay {} {} --tx {tx_hash} --mode execute",
        format_src_flag(src_rpc),
        format_dest_flag(dest_rpc)
    );
}

/// Format a human-friendly RPC label for logs.
fn format_rpc(rpc: &ResolvedRpc) -> String {
    rpc.alias.clone().unwrap_or_else(|| rpc.url.clone())
}

/// Format the CLI flag used to select a single RPC.
fn format_rpc_flag(rpc: &ResolvedRpc) -> String {
    if let Some(alias) = rpc.alias.as_deref() {
        format!("--chain {alias}")
    } else {
        format!("--rpc {}", rpc.url)
    }
}

/// Format the CLI flag used to select the source RPC.
fn format_src_flag(rpc: &ResolvedRpc) -> String {
    if let Some(alias) = rpc.alias.as_deref() {
        format!("--chain-src {alias}")
    } else {
        format!("--rpc-src {}", rpc.url)
    }
}

/// Format the CLI flag used to select the destination RPC.
fn format_dest_flag(rpc: &ResolvedRpc) -> String {
    if let Some(alias) = rpc.alias.as_deref() {
        format!("--chain-dest {alias}")
    } else {
        format!("--rpc-dest {}", rpc.url)
    }
}

/// Fetch the bundle status value from the handler contract.
async fn fetch_bundle_status(
    client: &RpcClient,
    handler: Address,
    bundle_hash: B256,
) -> Result<u8> {
    let call = encode_bundle_status_call(bundle_hash);
    let data = eth_call(client, handler, call).await?;
    crate::abi::decode_bundle_status(data)
}

/// Render a bundle status enum into a readable string.
fn status_string(value: u8) -> &'static str {
    match value {
        0 => "Unreceived",
        1 => "Verified",
        2 => "FullyExecuted",
        3 => "Unbundled",
        _ => "Unknown",
    }
}
