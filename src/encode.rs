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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    // Helper: construct a known address from a fixed byte pattern.
    fn addr(byte: u8) -> Address {
        Address::from([byte; 20])
    }

    // -------------------------------------------------------------------------
    // encode_evm_v1_with_address
    // -------------------------------------------------------------------------

    #[test]
    fn encode_with_address_starts_with_header() {
        let bytes = encode_evm_v1_with_address(U256::from(1u64), addr(0xAB));
        assert_eq!(&bytes[0..4], &EVM_V1_HEADER, "first 4 bytes must be EVM_V1_HEADER");
    }

    #[test]
    fn encode_with_address_chain_id_1() {
        // chain_id = 1  →  chain_ref = [0x01]  (1 byte)
        let chain_id = U256::from(1u64);
        let address = addr(0x11);
        let bytes = encode_evm_v1_with_address(chain_id, address);

        // layout: [header(4)] [chain_len(1)] [chain_bytes(1)] [addr_len(1)=20] [addr(20)]
        assert_eq!(bytes.len(), 4 + 1 + 1 + 1 + 20);
        assert_eq!(bytes[4], 1u8, "chain_len must be 1 for chain_id=1");
        assert_eq!(bytes[5], 0x01u8, "chain_byte must be 0x01");
        assert_eq!(bytes[6], 20u8, "addr_len must be 20");
        assert_eq!(&bytes[7..27], address.as_slice());
    }

    #[test]
    fn encode_with_address_chain_id_256() {
        // chain_id = 256 = 0x0100  →  chain_ref = [0x01, 0x00]  (2 bytes)
        let chain_id = U256::from(256u64);
        let address = addr(0x22);
        let bytes = encode_evm_v1_with_address(chain_id, address);

        assert_eq!(bytes[4], 2u8, "chain_len must be 2 for chain_id=256");
        assert_eq!(bytes[5], 0x01u8);
        assert_eq!(bytes[6], 0x00u8);
        assert_eq!(bytes[7], 20u8, "addr_len must be 20");
    }

    #[test]
    fn encode_with_address_large_chain_id() {
        // chain_id = u64::MAX = 0xFFFFFFFFFFFFFFFF  →  8 bytes
        let chain_id = U256::from(u64::MAX);
        let address = addr(0x33);
        let bytes = encode_evm_v1_with_address(chain_id, address);

        assert_eq!(bytes[4], 8u8, "chain_len must be 8 for u64::MAX");
        assert_eq!(bytes[5..13], [0xFF; 8]);
        assert_eq!(bytes[13], 20u8);
    }

    #[test]
    fn encode_with_address_known_zksync_chain_324() {
        // chain_id = 324 = 0x0144  →  2 bytes
        let chain_id = U256::from(324u64);
        let address = addr(0x44);
        let bytes = encode_evm_v1_with_address(chain_id, address);

        assert_eq!(bytes[4], 2u8);
        assert_eq!(bytes[5], 0x01u8);
        assert_eq!(bytes[6], 0x44u8);
        assert_eq!(bytes[7], 20u8);
        assert_eq!(&bytes[8..28], address.as_slice());
    }

    // -------------------------------------------------------------------------
    // encode_evm_v1_chain_only
    // -------------------------------------------------------------------------

    #[test]
    fn encode_chain_only_addr_len_is_zero() {
        let bytes = encode_evm_v1_chain_only(U256::from(1u64));
        // last byte is addr_len == 0
        assert_eq!(*bytes.last().unwrap(), 0u8, "addr_len must be 0 for chain-only");
    }

    #[test]
    fn encode_chain_only_header_and_chain_bytes() {
        let chain_id = U256::from(1u64);
        let bytes = encode_evm_v1_chain_only(chain_id);
        assert_eq!(&bytes[0..4], &EVM_V1_HEADER);
        assert_eq!(bytes[4], 1u8, "chain_len = 1");
        assert_eq!(bytes[5], 0x01u8, "chain byte = 0x01");
        assert_eq!(bytes[6], 0u8, "addr_len = 0");
        assert_eq!(bytes.len(), 7);
    }

    #[test]
    fn encode_chain_only_no_address_bytes() {
        // Total length must be 4 (header) + 1 (chain_len) + chain_bytes + 1 (addr_len=0)
        let chain_id = U256::from(1u64);
        let chain_bytes = 1usize;
        let bytes = encode_evm_v1_chain_only(chain_id);
        assert_eq!(bytes.len(), 4 + 1 + chain_bytes + 1);
    }

    // -------------------------------------------------------------------------
    // encode_evm_v1_address_only
    // -------------------------------------------------------------------------

    #[test]
    fn encode_address_only_starts_with_address_only_header() {
        let bytes = encode_evm_v1_address_only(addr(0x55));
        assert_eq!(&bytes[0..5], &EVM_V1_ADDRESS_ONLY_HEADER);
    }

    #[test]
    fn encode_address_only_structure() {
        let address = addr(0x66);
        let bytes = encode_evm_v1_address_only(address);
        // layout: [header(5)] [addr_len(1)=20] [addr(20)]
        assert_eq!(bytes.len(), 5 + 1 + 20);
        assert_eq!(bytes[5], 20u8, "addr_len byte must be 20");
        assert_eq!(&bytes[6..26], address.as_slice());
    }

    #[test]
    fn encode_address_only_no_chain_bytes() {
        // The 5-byte header has chain_len=0 embedded (byte index 4 == 0x00)
        let bytes = encode_evm_v1_address_only(addr(0x77));
        assert_eq!(bytes[4], 0x00u8, "chain_len byte must be 0x00 (no chain)");
    }

    // -------------------------------------------------------------------------
    // decode_evm_v1_address — round-trips
    // -------------------------------------------------------------------------

    #[test]
    fn round_trip_chain_and_address_chain_id_1() {
        let chain_id = U256::from(1u64);
        let address = addr(0xAA);
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(address));
    }

    #[test]
    fn round_trip_chain_and_address_chain_id_324() {
        let chain_id = U256::from(324u64);
        let address = address!("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(address));
    }

    #[test]
    fn round_trip_chain_and_address_u64_max() {
        let chain_id = U256::from(u64::MAX);
        let address = addr(0xBB);
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(address));
    }

    #[test]
    fn round_trip_chain_only() {
        let chain_id = U256::from(1u64);
        let encoded = encode_evm_v1_chain_only(chain_id);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, None, "chain-only must decode to no address");
    }

    #[test]
    fn round_trip_chain_id_zero() {
        // chain_id = 0 → to_chain_reference returns [0x00]
        let chain_id = U256::ZERO;
        let address = addr(0xCC);
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, U256::ZERO);
        assert_eq!(decoded_addr, Some(address));
    }

    #[test]
    fn round_trip_chain_id_255() {
        let chain_id = U256::from(255u64);
        let address = addr(0xDD);
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, chain_id);
        assert_eq!(decoded_addr, Some(address));
    }

    #[test]
    fn round_trip_u256_max() {
        let chain_id = U256::MAX;
        let address = addr(0xEE);
        let encoded = encode_evm_v1_with_address(chain_id, address);
        let (decoded_chain, decoded_addr) = decode_evm_v1_address(&encoded).unwrap();
        assert_eq!(decoded_chain, U256::MAX);
        assert_eq!(decoded_addr, Some(address));
    }

    // -------------------------------------------------------------------------
    // decode_evm_v1_address — error cases
    // -------------------------------------------------------------------------

    #[test]
    fn decode_error_too_short() {
        let short = Bytes::from(vec![0u8; 5]); // needs at least 6
        assert!(decode_evm_v1_address(&short).is_err(), "must fail when data < 6 bytes");
    }

    #[test]
    fn decode_error_wrong_header() {
        let mut data = vec![0xFF, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00];
        data.extend_from_slice(&[0u8; 20]);
        let bytes = Bytes::from(data);
        assert!(decode_evm_v1_address(&bytes).is_err(), "must fail on wrong header byte");
    }

    #[test]
    fn decode_error_truncated_chain_bytes() {
        // header OK, chain_len=8 but only 2 bytes of chain data follow, then no addr_len
        let mut data = EVM_V1_HEADER.to_vec();
        data.push(8u8); // chain_len = 8
        data.extend_from_slice(&[0x01, 0x00]); // only 2 bytes instead of 8
        let bytes = Bytes::from(data);
        assert!(decode_evm_v1_address(&bytes).is_err());
    }

    #[test]
    fn decode_error_truncated_address_bytes() {
        // valid header + chain, addr_len=20 but only 10 address bytes provided
        let chain_id = U256::from(1u64);
        let address = addr(0xFF);
        let mut encoded = encode_evm_v1_with_address(chain_id, address).to_vec();
        // truncate the last 10 bytes of the address
        let new_len = encoded.len() - 10;
        encoded.truncate(new_len);
        let bytes = Bytes::from(encoded);
        assert!(decode_evm_v1_address(&bytes).is_err(), "must fail on truncated address");
    }

    // -------------------------------------------------------------------------
    // encode_interop_call_value — ABI selector + value encoding
    // -------------------------------------------------------------------------

    #[test]
    fn encode_interop_call_value_selector() {
        // selector = keccak256("interopCallValue(uint256)")[0..4]
        let sig = b"interopCallValue(uint256)";
        let expected_selector: [u8; 4] = keccak256(sig)[0..4].try_into().unwrap();

        let encoded = encode_interop_call_value(U256::ZERO);
        assert_eq!(encoded.len(), 36, "ABI-encoded call must be 4 (selector) + 32 (uint256) bytes");
        assert_eq!(&encoded[0..4], &expected_selector, "selector mismatch for interopCallValue");
    }

    #[test]
    fn encode_interop_call_value_encodes_value_in_last_32_bytes() {
        let value = U256::from(0xDEAD_BEEFu64);
        let encoded = encode_interop_call_value(value);
        // The uint256 is ABI-encoded big-endian in the last 32 bytes
        let mut expected = [0u8; 32];
        let value_bytes = value.to_be_bytes::<32>();
        expected.copy_from_slice(&value_bytes);
        assert_eq!(&encoded[4..36], &expected);
    }

    #[test]
    fn encode_interop_call_value_zero() {
        let encoded = encode_interop_call_value(U256::ZERO);
        assert_eq!(&encoded[4..36], &[0u8; 32]);
    }

    #[test]
    fn encode_interop_call_value_max() {
        let encoded = encode_interop_call_value(U256::MAX);
        assert_eq!(&encoded[4..36], &[0xFFu8; 32]);
    }

    // -------------------------------------------------------------------------
    // encode_indirect_call — ABI selector + value encoding
    // -------------------------------------------------------------------------

    #[test]
    fn encode_indirect_call_selector() {
        let sig = b"indirectCall(uint256)";
        let expected_selector: [u8; 4] = keccak256(sig)[0..4].try_into().unwrap();

        let encoded = encode_indirect_call(U256::ZERO);
        assert_eq!(encoded.len(), 36);
        assert_eq!(&encoded[0..4], &expected_selector, "selector mismatch for indirectCall");
    }

    #[test]
    fn encode_indirect_call_different_selector_from_interop_call_value() {
        let a = encode_interop_call_value(U256::from(1u64));
        let b = encode_indirect_call(U256::from(1u64));
        assert_ne!(&a[0..4], &b[0..4], "selectors must differ between the two functions");
    }

    #[test]
    fn encode_indirect_call_value_in_last_32_bytes() {
        let value = U256::from(42u64);
        let encoded = encode_indirect_call(value);
        let expected: [u8; 32] = value.to_be_bytes::<32>();
        assert_eq!(&encoded[4..36], &expected);
    }

    // -------------------------------------------------------------------------
    // encode_asset_id — determinism and distinctness
    // -------------------------------------------------------------------------

    #[test]
    fn encode_asset_id_deterministic() {
        let chain_id = U256::from(1u64);
        let token = addr(0x01);
        let vault = addr(0x02);
        let a = encode_asset_id(chain_id, token, vault);
        let b = encode_asset_id(chain_id, token, vault);
        assert_eq!(a, b, "encode_asset_id must be deterministic");
    }

    #[test]
    fn encode_asset_id_is_32_bytes() {
        let result = encode_asset_id(U256::from(1u64), addr(0x01), addr(0x02));
        assert_eq!(result.len(), 32, "keccak256 output must be 32 bytes");
    }

    #[test]
    fn encode_asset_id_different_chain_produces_different_hash() {
        let token = addr(0x01);
        let vault = addr(0x02);
        let a = encode_asset_id(U256::from(1u64), token, vault);
        let b = encode_asset_id(U256::from(2u64), token, vault);
        assert_ne!(a, b);
    }

    #[test]
    fn encode_asset_id_different_token_produces_different_hash() {
        let chain_id = U256::from(1u64);
        let vault = addr(0x02);
        let a = encode_asset_id(chain_id, addr(0x10), vault);
        let b = encode_asset_id(chain_id, addr(0x20), vault);
        assert_ne!(a, b);
    }

    #[test]
    fn encode_asset_id_different_vault_produces_different_hash() {
        let chain_id = U256::from(1u64);
        let token = addr(0x01);
        let a = encode_asset_id(chain_id, token, addr(0x10));
        let b = encode_asset_id(chain_id, token, addr(0x20));
        assert_ne!(a, b);
    }

    // -------------------------------------------------------------------------
    // parse_permissionless_address
    // -------------------------------------------------------------------------

    #[test]
    fn parse_permissionless_returns_none() {
        let result = parse_permissionless_address("permissionless").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_permissionless_valid_address() {
        let addr_str = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let result = parse_permissionless_address(addr_str).unwrap();
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(
            format!("{parsed:#x}"),
            addr_str,
            "parsed address must match the input"
        );
    }

    #[test]
    fn parse_permissionless_invalid_address_returns_err() {
        let result = parse_permissionless_address("not-an-address");
        assert!(result.is_err(), "invalid address string must return Err");
    }

    #[test]
    fn parse_permissionless_partial_hex_returns_err() {
        let result = parse_permissionless_address("0xDEAD");
        assert!(result.is_err(), "too-short hex must return Err");
    }

    #[test]
    fn parse_permissionless_case_sensitive_sentinel() {
        // "Permissionless" (capital P) must NOT match — it's not the sentinel
        let result = parse_permissionless_address("Permissionless");
        assert!(result.is_err(), "sentinel is case-sensitive");
    }

    // -------------------------------------------------------------------------
    // parse_payload
    // -------------------------------------------------------------------------

    #[test]
    fn parse_payload_both_set_returns_err() {
        let path = std::path::Path::new("/dev/null");
        let result = parse_payload(Some("0x01"), Some(path));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("both"), "error must mention --payload and --payload-file conflict");
    }

    #[test]
    fn parse_payload_neither_set_returns_err() {
        let result = parse_payload(None, None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("payload required"), "error must mention payload is required");
    }

    #[test]
    fn parse_payload_valid_hex_payload() {
        let result = parse_payload(Some("0xdeadbeef"), None).unwrap();
        assert_eq!(result.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_payload_valid_hex_without_prefix() {
        let result = parse_payload(Some("deadbeef"), None).unwrap();
        assert_eq!(result.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn parse_payload_invalid_hex_returns_err() {
        let result = parse_payload(Some("0xzzzz"), None);
        assert!(result.is_err(), "invalid hex must return Err");
    }

    #[test]
    fn parse_payload_empty_hex_returns_empty_bytes() {
        let result = parse_payload(Some("0x"), None).unwrap();
        assert!(result.is_empty(), "0x with no data must decode to empty bytes");
    }

    // -------------------------------------------------------------------------
    // to_chain_reference — tested indirectly via encode/decode round-trips
    // at boundary values already covered above; add a structural check here.
    // -------------------------------------------------------------------------

    #[test]
    fn chain_reference_strips_leading_zeros() {
        // chain_id = 1 must produce exactly 1 byte (not 32 zero-padded bytes)
        let encoded = encode_evm_v1_chain_only(U256::from(1u64));
        // byte[4] is chain_len
        assert_eq!(encoded[4], 1u8, "chain reference for 1 should be 1 byte, not 32");
    }

    #[test]
    fn chain_reference_u256_max_is_32_bytes() {
        let encoded = encode_evm_v1_chain_only(U256::MAX);
        assert_eq!(encoded[4], 32u8, "chain reference for U256::MAX must be 32 bytes");
        // all 32 chain bytes must be 0xFF
        assert_eq!(&encoded[5..37], &[0xFFu8; 32]);
    }

    #[test]
    fn chain_reference_zero_is_single_zero_byte() {
        // to_chain_reference(0) returns [0x00] — a single zero byte
        let encoded = encode_evm_v1_chain_only(U256::ZERO);
        assert_eq!(encoded[4], 1u8, "chain_len must be 1 even for chain_id=0");
        assert_eq!(encoded[5], 0x00u8, "the single byte must be 0x00");
    }
}
