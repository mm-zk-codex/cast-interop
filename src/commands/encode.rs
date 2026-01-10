use crate::cli::{Encode7930Args, EncodeAssetIdArgs, EncodeAttrsArgs};
use crate::config::Config;
use crate::encode::{
    encode_asset_id, encode_evm_v1_address_only, encode_evm_v1_chain_only,
    encode_evm_v1_with_address, encode_execution_address, encode_indirect_call,
    encode_interop_call_value, encode_unbundler_address, parse_permissionless_address,
    DEFAULT_NATIVE_TOKEN_VAULT,
};
use crate::types::{format_hex, parse_address, parse_u256, AddressBook};
use alloy_primitives::Bytes;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EncodeAttrsOutput {
    attributes: Vec<String>,
}

pub async fn run_7930(
    args: Encode7930Args,
    _config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let output = if let Some(address_only) = args.address_only {
        if args.chain_id.is_some() || args.address.is_some() {
            anyhow::bail!("--address-only cannot be combined with --chain-id or --address");
        }
        let address = parse_address(&address_only)?;
        encode_evm_v1_address_only(address)
    } else if let (Some(chain_id), Some(address)) = (args.chain_id.clone(), args.address) {
        let chain_id = parse_u256(&chain_id)?;
        let address = parse_address(&address)?;
        encode_evm_v1_with_address(chain_id, address)
    } else if let Some(chain_id) = args.chain_id {
        let chain_id = parse_u256(&chain_id)?;
        encode_evm_v1_chain_only(chain_id)
    } else {
        anyhow::bail!("set --chain-id (with optional --address) or --address-only");
    };

    println!("{}", format_hex(output.as_ref()));
    Ok(())
}

pub async fn run_attrs(
    args: EncodeAttrsArgs,
    _config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let mut attributes: Vec<Bytes> = Vec::new();
    if let Some(value) = args.interop_value {
        let parsed = parse_u256(&value)?;
        attributes.push(encode_interop_call_value(parsed));
    }
    if let Some(value) = args.indirect {
        let parsed = parse_u256(&value)?;
        attributes.push(encode_indirect_call(parsed));
    }
    if let Some(value) = args.execution_address {
        let encoded = match parse_permissionless_address(&value)? {
            None => Bytes::new(),
            Some(addr) => encode_evm_v1_address_only(addr),
        };
        attributes.push(encode_execution_address(encoded));
    }
    if let Some(value) = args.unbundler {
        let addr = parse_address(&value)?;
        attributes.push(encode_unbundler_address(encode_evm_v1_address_only(addr)));
    }

    let output = EncodeAttrsOutput {
        attributes: attributes
            .iter()
            .map(|value| format_hex(value.as_ref()))
            .collect(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for value in output.attributes {
            println!("{value}");
        }
    }
    Ok(())
}

pub async fn run_asset_id(
    args: EncodeAssetIdArgs,
    _config: Config,
    _addresses: AddressBook,
) -> Result<()> {
    let chain_id = parse_u256(&args.chain_id)?;
    let token = parse_address(&args.token)?;
    let vault = parse_address(
        args.native_token_vault
            .as_deref()
            .unwrap_or(DEFAULT_NATIVE_TOKEN_VAULT),
    )?;

    let asset_id = encode_asset_id(chain_id, token, vault);
    println!("{}", format_hex(asset_id.as_ref()));
    Ok(())
}
