use crate::types::{bytes_from_hex, parse_address};
use alloy_primitives::{keccak256, Address, Bytes, U256};
use alloy_sol_types::SolValue;
use anyhow::{anyhow, Result};

alloy_sol_types::sol! {
    function interopCallValue(uint256 _interopCallValue);
    function indirectCall(uint256 _indirectCallMessageValue);
    function executionAddress(bytes _executionAddress);
    function unbundlerAddress(bytes _unbundlerAddress);
}

pub const EVM_V1_HEADER: [u8; 4] = [0x00, 0x01, 0x00, 0x00];
pub const EVM_V1_ADDRESS_ONLY_HEADER: [u8; 5] = [0x00, 0x01, 0x00, 0x00, 0x00];
pub const DEFAULT_NATIVE_TOKEN_VAULT: &str = "0x0000000000000000000000000000000000010004";

pub fn encode_evm_v1_with_address(chain_id: U256, address: Address) -> Bytes {
    let chain_ref = to_chain_reference(chain_id);
    let mut out = Vec::with_capacity(4 + 1 + chain_ref.len() + 1 + 20);
    out.extend_from_slice(&EVM_V1_HEADER);
    out.push(chain_ref.len() as u8);
    out.extend_from_slice(&chain_ref);
    out.push(20);
    out.extend_from_slice(address.as_slice());
    Bytes::from(out)
}

pub fn encode_evm_v1_chain_only(chain_id: U256) -> Bytes {
    let chain_ref = to_chain_reference(chain_id);
    let mut out = Vec::with_capacity(4 + 1 + chain_ref.len() + 1);
    out.extend_from_slice(&EVM_V1_HEADER);
    out.push(chain_ref.len() as u8);
    out.extend_from_slice(&chain_ref);
    out.push(0);
    Bytes::from(out)
}

pub fn encode_evm_v1_address_only(address: Address) -> Bytes {
    let mut out = Vec::with_capacity(5 + 20);
    out.extend_from_slice(&EVM_V1_ADDRESS_ONLY_HEADER);
    out.push(20);
    out.extend_from_slice(address.as_slice());
    Bytes::from(out)
}

pub fn encode_interop_call_value(value: U256) -> Bytes {
    let call = interopCallValueCall {
        _interopCallValue: value,
    };
    Bytes::from(call.abi_encode())
}

pub fn encode_indirect_call(value: U256) -> Bytes {
    let call = indirectCallCall {
        _indirectCallMessageValue: value,
    };
    Bytes::from(call.abi_encode())
}

pub fn encode_execution_address(value: Bytes) -> Bytes {
    let call = executionAddressCall {
        _executionAddress: value,
    };
    Bytes::from(call.abi_encode())
}

pub fn encode_unbundler_address(value: Bytes) -> Bytes {
    let call = unbundlerAddressCall {
        _unbundlerAddress: value,
    };
    Bytes::from(call.abi_encode())
}

pub fn parse_payload(
    payload: Option<&str>,
    payload_file: Option<&std::path::Path>,
) -> Result<Bytes> {
    match (payload, payload_file) {
        (Some(_), Some(_)) => anyhow::bail!("cannot set both --payload and --payload-file"),
        (Some(payload), None) => bytes_from_hex(payload).map(|b| b.0),
        (None, Some(path)) => {
            let contents = std::fs::read_to_string(path)?;
            bytes_from_hex(&contents).map(|b| b.0)
        }
        (None, None) => anyhow::bail!("payload required (set --payload or --payload-file)"),
    }
}

pub fn parse_permissionless_address(value: &str) -> Result<Option<Address>> {
    if value == "permissionless" {
        return Ok(None);
    }
    parse_address(value).map(Some)
}

pub fn encode_asset_id(chain_id: U256, token: Address, native_token_vault: Address) -> Bytes {
    let encoded = (chain_id, native_token_vault, token).abi_encode();
    Bytes::from(keccak256(encoded).to_vec())
}

pub fn decode_evm_v1_address(data: &Bytes) -> Result<(U256, Option<Address>)> {
    let bytes = data.as_ref();
    if bytes.len() < 6 {
        anyhow::bail!("erc-7930 data too short");
    }
    if bytes[0..4] != EVM_V1_HEADER {
        anyhow::bail!("unsupported ERC-7930 header");
    }
    let chain_len = bytes[4] as usize;
    let chain_start = 5;
    let chain_end = chain_start + chain_len;
    if bytes.len() < chain_end + 1 {
        anyhow::bail!("erc-7930 data missing address length");
    }
    let chain_ref = &bytes[chain_start..chain_end];
    let addr_len = bytes[chain_end] as usize;
    let addr_start = chain_end + 1;
    let addr_end = addr_start + addr_len;
    if bytes.len() < addr_end {
        anyhow::bail!("erc-7930 data truncated");
    }
    let chain_id = if chain_len == 0 {
        U256::ZERO
    } else {
        U256::from_be_slice(chain_ref)
    };
    let address = if addr_len == 0 {
        None
    } else if addr_len == 20 {
        Some(Address::from_slice(&bytes[addr_start..addr_end]))
    } else {
        anyhow::bail!("unsupported address length {addr_len}");
    };
    Ok((chain_id, address))
}

fn to_chain_reference(chain_id: U256) -> Vec<u8> {
    if chain_id == U256::ZERO {
        return vec![0u8];
    }
    let mut bytes = chain_id.to_be_bytes::<32>().to_vec();
    while bytes.first() == Some(&0) {
        bytes.remove(0);
    }
    if bytes.is_empty() {
        vec![0u8]
    } else {
        bytes
    }
}
