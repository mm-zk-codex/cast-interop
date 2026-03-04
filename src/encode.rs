use crate::types::{bytes_from_hex, parse_address};
use alloy_primitives::{keccak256, Address, Bytes, U256};
use alloy_sol_types::{SolCall, SolValue};
use anyhow::Result;

alloy_sol_types::sol! {
    function interopCallValue(uint256 _interopCallValue);
    function indirectCall(uint256 _indirectCallMessageValue);
    function executionAddress(bytes _executionAddress);
    function unbundlerAddress(bytes _unbundlerAddress);
}

pub const EVM_V1_HEADER: [u8; 4] = [0x00, 0x01, 0x00, 0x00];
pub const EVM_V1_ADDRESS_ONLY_HEADER: [u8; 5] = [0x00, 0x01, 0x00, 0x00, 0x00];
pub const DEFAULT_NATIVE_TOKEN_VAULT: &str = "0x0000000000000000000000000000000000010004";

/// Encode a chain+address pair using the ERC-7930 v1 format.
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

/// Encode a chain-only ERC-7930 v1 reference (no address).
pub fn encode_evm_v1_chain_only(chain_id: U256) -> Bytes {
    let chain_ref = to_chain_reference(chain_id);
    let mut out = Vec::with_capacity(4 + 1 + chain_ref.len() + 1);
    out.extend_from_slice(&EVM_V1_HEADER);
    out.push(chain_ref.len() as u8);
    out.extend_from_slice(&chain_ref);
    out.push(0);
    Bytes::from(out)
}

/// Encode an address-only ERC-7930 v1 reference (no chain ID).
pub fn encode_evm_v1_address_only(address: Address) -> Bytes {
    let mut out = Vec::with_capacity(5 + 20);
    out.extend_from_slice(&EVM_V1_ADDRESS_ONLY_HEADER);
    out.push(20);
    out.extend_from_slice(address.as_slice());
    Bytes::from(out)
}

/// Encode the interop call value attribute.
pub fn encode_interop_call_value(value: U256) -> Bytes {
    let call = interopCallValueCall {
        _interopCallValue: value,
    };
    Bytes::from(call.abi_encode())
}

/// Encode the indirect call value attribute.
pub fn encode_indirect_call(value: U256) -> Bytes {
    let call = indirectCallCall {
        _indirectCallMessageValue: value,
    };
    Bytes::from(call.abi_encode())
}

/// Encode the execution address attribute.
pub fn encode_execution_address(value: Bytes) -> Bytes {
    let call = executionAddressCall {
        _executionAddress: value,
    };
    Bytes::from(call.abi_encode())
}

/// Encode the unbundler address attribute.
pub fn encode_unbundler_address(value: Bytes) -> Bytes {
    let call = unbundlerAddressCall {
        _unbundlerAddress: value,
    };
    Bytes::from(call.abi_encode())
}

/// Parse payload input from --payload or --payload-file.
///
/// Ensures only one input source is set.
pub fn parse_payload(
    payload: Option<&str>,
    payload_file: Option<&std::path::Path>,
) -> Result<Bytes> {
    match (payload, payload_file) {
        (Some(_), Some(_)) => anyhow::bail!("cannot set both --payload and --payload-file"),
        (Some(payload), None) => bytes_from_hex(payload),
        (None, Some(path)) => {
            let contents = std::fs::read_to_string(path)?;
            bytes_from_hex(&contents)
        }
        (None, None) => anyhow::bail!("payload required (set --payload or --payload-file)"),
    }
}

/// Parse an address or the \"permissionless\" sentinel.
///
/// Returns None when permissionless is requested.
pub fn parse_permissionless_address(value: &str) -> Result<Option<Address>> {
    if value == "permissionless" {
        return Ok(None);
    }
    parse_address(value).map(Some)
}

/// Compute the assetId hash for a token and vault on a chain.
///
/// This is keccak(chainId, nativeTokenVault, token).
pub fn encode_asset_id(chain_id: U256, token: Address, native_token_vault: Address) -> Bytes {
    let encoded = (chain_id, native_token_vault, token).abi_encode();
    Bytes::from(keccak256(encoded).to_vec())
}

/// Decode an ERC-7930 v1 chain/address reference.
///
/// Returns the chain ID and optional address.
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, Address, Bytes, U256};

    #[test]
    fn encode_decode_with_address_roundtrip() {
        let chain_id = U256::from(1u64);
        let addr = address!("1234567890123456789012345678901234567890");
        let encoded = encode_evm_v1_with_address(chain_id, addr);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(addr));
    }

    #[test]
    fn encode_decode_large_chain_id() {
        // 300 = 0x012c, requires 2 bytes
        let chain_id = U256::from(300u64);
        let addr = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let encoded = encode_evm_v1_with_address(chain_id, addr);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(addr));
    }

    #[test]
    fn encode_decode_chain_only_no_address() {
        let chain_id = U256::from(42u64);
        let encoded = encode_evm_v1_chain_only(chain_id);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, None);
    }

    #[test]
    fn chain_reference_is_minimal_single_byte() {
        // chain_id=1 → 1 byte chain ref: header(4) + len(1) + chain(1) + addr_len(1) + addr(20) = 27
        let chain_id = U256::from(1u64);
        let encoded = encode_evm_v1_with_address(
            chain_id,
            address!("0000000000000000000000000000000000000001"),
        );
        assert_eq!(encoded.len(), 27);
    }

    #[test]
    fn encode_evm_v1_address_only_layout() {
        let addr = address!("1234567890123456789012345678901234567890");
        let encoded = encode_evm_v1_address_only(addr);
        // EVM_V1_ADDRESS_ONLY_HEADER(5) + addr_len(1) + addr(20) = 26
        assert_eq!(encoded.len(), 26);
        assert_eq!(&encoded[..5], &EVM_V1_ADDRESS_ONLY_HEADER);
        assert_eq!(encoded[5], 20);
        assert_eq!(&encoded[6..], addr.as_slice());
    }

    #[test]
    fn encode_asset_id_is_deterministic() {
        let chain_id = U256::from(1u64);
        let token = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let vault: Address = DEFAULT_NATIVE_TOKEN_VAULT.parse().unwrap();
        let id1 = encode_asset_id(chain_id, token, vault);
        let id2 = encode_asset_id(chain_id, token, vault);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 32);
    }

    #[test]
    fn encode_asset_id_changes_with_chain() {
        let token = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let vault: Address = DEFAULT_NATIVE_TOKEN_VAULT.parse().unwrap();
        let id1 = encode_asset_id(U256::from(1u64), token, vault);
        let id2 = encode_asset_id(U256::from(2u64), token, vault);
        assert_ne!(id1, id2);
    }

    #[test]
    fn encode_asset_id_changes_with_token() {
        let chain_id = U256::from(1u64);
        let vault: Address = DEFAULT_NATIVE_TOKEN_VAULT.parse().unwrap();
        let id1 = encode_asset_id(
            chain_id,
            address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            vault,
        );
        let id2 = encode_asset_id(
            chain_id,
            address!("dAC17F958D2ee523a2206206994597C13D831ec7"),
            vault,
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn parse_permissionless_sentinel_returns_none() {
        let result = parse_permissionless_address("permissionless").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_permissionless_valid_address_returns_some() {
        let result =
            parse_permissionless_address("0x1234567890123456789012345678901234567890").unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn parse_permissionless_invalid_address_errors() {
        assert!(parse_permissionless_address("notanaddress").is_err());
    }

    #[test]
    fn parse_payload_both_flags_errors() {
        let result = parse_payload(Some("0xdead"), Some(std::path::Path::new("/tmp/x")));
        assert!(result.is_err());
    }

    #[test]
    fn parse_payload_neither_flag_errors() {
        assert!(parse_payload(None, None).is_err());
    }

    #[test]
    fn parse_payload_hex_with_0x_prefix() {
        let result = parse_payload(Some("0xdeadbeef"), None).unwrap();
        assert_eq!(result.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_payload_hex_without_prefix() {
        let result = parse_payload(Some("deadbeef"), None).unwrap();
        assert_eq!(result.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn encode_interop_call_value_has_4_byte_selector() {
        let encoded = encode_interop_call_value(U256::from(1000u64));
        // 4-byte selector + 32-byte uint256
        assert_eq!(encoded.len(), 36);
    }

    #[test]
    fn encode_indirect_call_has_4_byte_selector() {
        let encoded = encode_indirect_call(U256::from(500u64));
        assert_eq!(encoded.len(), 36);
    }

    #[test]
    fn encode_call_value_and_indirect_have_different_selectors() {
        let a = encode_interop_call_value(U256::from(1u64));
        let b = encode_indirect_call(U256::from(1u64));
        assert_ne!(&a[..4], &b[..4]);
    }

    #[test]
    fn decode_evm_v1_address_too_short_errors() {
        let short = Bytes::from(vec![0u8; 4]);
        assert!(decode_evm_v1_address(&short).is_err());
    }

    #[test]
    fn decode_evm_v1_address_wrong_header_errors() {
        // Wrong header bytes
        let data = Bytes::from(vec![0xFF, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00]);
        assert!(decode_evm_v1_address(&data).is_err());
    }
}

/// Convert a chain ID to a minimal big-endian byte representation.
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
