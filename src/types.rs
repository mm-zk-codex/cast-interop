use alloy_primitives::{address, Address, Bytes, B256, U256};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const DEFAULT_INTEROP_CENTER: &str = "0x000000000000000000000000000000000001000d";
pub const DEFAULT_INTEROP_HANDLER: &str = "0x000000000000000000000000000000000001000e";
pub const DEFAULT_INTEROP_ROOT_STORAGE: &str = "0x0000000000000000000000000000000000010008";
pub const DEFAULT_ASSET_ROUTER: &str = "0x0000000000000000000000000000000000010003";
pub const BUNDLE_IDENTIFIER: u8 = 0x01;

pub const L1_SENDER_ADDRESS: Address = address!("0000000000000000000000000000000000008008");
pub const INTEROP_CENTER_ADDRESS: Address = address!("000000000000000000000000000000000001000d");

#[derive(Clone, Debug)]
pub struct AddressBook {
    pub interop_center: Address,
    pub interop_handler: Address,
    pub interop_root_storage: Address,
}

impl AddressBook {
    pub fn from_config_and_flags(
        config: &crate::config::Config,
        center: Option<&str>,
        handler: Option<&str>,
        root_storage: Option<&str>,
    ) -> Result<Self> {
        let center = center
            .map(|value| value.to_string())
            .or_else(|| config.addresses.as_ref()?.interop_center.clone())
            .unwrap_or_else(|| DEFAULT_INTEROP_CENTER.to_string());
        let handler = handler
            .map(|value| value.to_string())
            .or_else(|| config.addresses.as_ref()?.interop_handler.clone())
            .unwrap_or_else(|| DEFAULT_INTEROP_HANDLER.to_string());
        let root_storage = root_storage
            .map(|value| value.to_string())
            .or_else(|| config.addresses.as_ref()?.interop_root_storage.clone())
            .unwrap_or_else(|| DEFAULT_INTEROP_ROOT_STORAGE.to_string());

        Ok(Self {
            interop_center: parse_address(&center)?,
            interop_handler: parse_address(&handler)?,
            interop_root_storage: parse_address(&root_storage)?,
        })
    }
}

pub fn parse_address(value: &str) -> Result<Address> {
    Address::from_str(value).map_err(|err| anyhow!("invalid address {value}: {err}"))
}

pub fn parse_b256(value: &str) -> Result<B256> {
    B256::from_str(value).map_err(|err| anyhow!("invalid bytes32 {value}: {err}"))
}

pub fn parse_u256(value: &str) -> Result<U256> {
    U256::from_str(value).map_err(|err| anyhow!("invalid uint256 {value}: {err}"))
}

pub fn format_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

pub fn bytes_from_hex(value: &str) -> Result<Bytes> {
    let trimmed = value.trim();
    let value = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = hex::decode(value).map_err(|err| anyhow!("invalid hex {value}: {err}"))?;
    Ok(Bytes::from(bytes))
}

pub fn require_signer_or_dry_run(has_signer: bool, dry_run: bool, cmd: &str) -> Result<()> {
    if !has_signer && !dry_run {
        anyhow::bail!("{cmd} requires a signer or --dry-run");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofMessage {
    pub tx_number_in_batch: u64,
    pub sender: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageInclusionProof {
    pub chain_id: String,
    pub l1_batch_number: u64,
    pub l2_message_index: u64,
    pub root: String,
    pub message: ProofMessage,
    pub proof: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteropCallView {
    pub version: String,
    pub shadow_account: bool,
    pub to: String,
    pub from: String,
    pub value: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleAttributesView {
    pub execution_address: String,
    pub unbundler_address: String,
    pub use_fixed_fee: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteropBundleView {
    pub version: String,
    pub source_chain_id: String,
    pub destination_chain_id: String,
    pub interop_bundle_salt: String,
    pub calls: Vec<InteropCallView>,
    pub bundle_attributes: BundleAttributesView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleExtractOutput {
    pub bundle_hash: String,
    pub encoded_bundle_hex: String,
    pub bundle: InteropBundleView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxShowOutput {
    pub tx_hash: String,
    pub bundle: Option<InteropBundleView>,
    pub bundle_hash: Option<String>,
    pub l2l1_msg_hash: Option<String>,
    pub interop_events: Vec<EventView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub name: String,
    pub address: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusOutput {
    pub bundle_hash: String,
    pub bundle_status: String,
    pub calls: Option<Vec<CallStatusView>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallStatusView {
    pub index: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySummary {
    pub source_chain_id: String,
    pub destination_chain_id: String,
    pub l1_batch_number: u64,
    pub l2_message_index: u64,
    pub bundle_hash: String,
    pub source_tx_hash: String,
    pub handler_tx_hash: Option<String>,
}

alloy_sol_types::sol! {
    struct InteropCall {
        bytes1 version;
        bool shadowAccount;
        address to;
        address from;
        uint256 value;
        bytes data;
    }

    struct BundleAttributes {
        bytes executionAddress;
        bytes unbundlerAddress;
        bool useFixedFee;
    }

    struct InteropBundle {
        bytes1 version;
        uint256 sourceChainId;
        uint256 destinationChainId;
        bytes32 interopBundleSalt;
        InteropCall[] calls;
        BundleAttributes bundleAttributes;
    }
}

pub fn u256_to_string(value: U256) -> String {
    value.to_string()
}

pub fn b256_to_hex(value: B256) -> String {
    format!("{value:#x}")
}

pub fn address_to_hex(value: Address) -> String {
    format!("{value:#x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // parse_address
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_address_valid_checksummed() {
        // EIP-55 checksummed address must be accepted.
        let addr = parse_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        assert!(addr.is_ok(), "checksummed address should parse");
    }

    #[test]
    fn parse_address_valid_lowercase() {
        let addr = parse_address("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed");
        assert!(addr.is_ok(), "lowercase hex address should parse");
    }

    #[test]
    fn parse_address_zero_address() {
        let addr = parse_address("0x0000000000000000000000000000000000000000").unwrap();
        assert_eq!(addr, Address::ZERO);
    }

    #[test]
    fn parse_address_known_default_constant() {
        // The DEFAULT_INTEROP_CENTER constant must round-trip through parse_address.
        let addr = parse_address(DEFAULT_INTEROP_CENTER);
        assert!(addr.is_ok(), "DEFAULT_INTEROP_CENTER should be a valid address");
    }

    #[test]
    fn parse_address_too_short() {
        let result = parse_address("0x1234");
        assert!(result.is_err(), "too-short address should fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid address"), "error should mention 'invalid address'");
    }

    #[test]
    fn parse_address_too_long() {
        let result = parse_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed00");
        assert!(result.is_err(), "too-long address should fail");
    }

    #[test]
    fn parse_address_bad_hex_chars() {
        let result = parse_address("0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ");
        assert!(result.is_err(), "non-hex chars should fail");
    }

    #[test]
    fn parse_address_missing_0x_prefix() {
        // alloy Address::from_str accepts addresses without the 0x prefix (treats raw hex as valid).
        // The important thing is that the result is a valid Address equivalent to the 0x-prefixed form.
        let with_prefix = parse_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed").unwrap();
        let without_prefix = parse_address("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed").unwrap();
        assert_eq!(with_prefix, without_prefix, "with and without 0x prefix should produce same address");
    }

    // ---------------------------------------------------------------------------
    // bytes_from_hex
    // ---------------------------------------------------------------------------

    #[test]
    fn bytes_from_hex_with_0x_prefix() {
        let bytes = bytes_from_hex("0xdeadbeef").unwrap();
        assert_eq!(bytes.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn bytes_from_hex_without_0x_prefix() {
        let bytes = bytes_from_hex("deadbeef").unwrap();
        assert_eq!(bytes.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn bytes_from_hex_empty_string() {
        let bytes = bytes_from_hex("").unwrap();
        assert!(bytes.is_empty(), "empty string should produce empty Bytes");
    }

    #[test]
    fn bytes_from_hex_0x_only() {
        // "0x" with nothing after the prefix should also yield empty bytes.
        let bytes = bytes_from_hex("0x").unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn bytes_from_hex_odd_length_error() {
        let result = bytes_from_hex("0xabc");
        assert!(result.is_err(), "odd-length hex should fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid hex"), "error message should mention 'invalid hex'");
    }

    #[test]
    fn bytes_from_hex_non_hex_chars() {
        let result = bytes_from_hex("0xGGGG");
        assert!(result.is_err(), "non-hex chars should fail");
    }

    #[test]
    fn bytes_from_hex_whitespace_trimmed() {
        // Leading/trailing whitespace must be stripped before parsing.
        let bytes = bytes_from_hex("  0xff00  ").unwrap();
        assert_eq!(bytes.as_ref(), &[0xff, 0x00]);
    }

    // ---------------------------------------------------------------------------
    // parse_u256
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_u256_valid_decimal_zero() {
        let v = parse_u256("0").unwrap();
        assert_eq!(v, U256::ZERO);
    }

    #[test]
    fn parse_u256_valid_decimal() {
        let v = parse_u256("1000000000000000000").unwrap(); // 1e18
        assert_eq!(v, U256::from(1_000_000_000_000_000_000u128));
    }

    #[test]
    fn parse_u256_max_value() {
        // U256::MAX in decimal should parse correctly.
        let max_str = U256::MAX.to_string();
        let v = parse_u256(&max_str).unwrap();
        assert_eq!(v, U256::MAX);
    }

    #[test]
    fn parse_u256_overflow() {
        // A number that exceeds U256::MAX must fail.
        // U256::MAX + 1 written in decimal.
        let overflow =
            "115792089237316195423570985008687907853269984665640564039457584007913129639936";
        let result = parse_u256(overflow);
        assert!(result.is_err(), "overflow value should fail to parse");
    }

    #[test]
    fn parse_u256_empty_string() {
        // alloy U256::from_str treats empty string as zero rather than erroring.
        // We document this behaviour: the result is U256::ZERO.
        let result = parse_u256("");
        match result {
            Ok(v) => assert_eq!(v, U256::ZERO, "empty string parses as zero per alloy semantics"),
            Err(_) => {} // also acceptable — either behaviour is documented here
        }
    }

    #[test]
    fn parse_u256_garbage_input() {
        let result = parse_u256("not_a_number");
        assert!(result.is_err(), "garbage input should fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid uint256"), "error should mention 'invalid uint256'");
    }

    #[test]
    fn parse_u256_hex_with_0x() {
        // alloy U256::from_str accepts "0x"-prefixed hex strings.
        let v = parse_u256("0xff").unwrap();
        assert_eq!(v, U256::from(255u64));
    }

    // ---------------------------------------------------------------------------
    // format_hex
    // ---------------------------------------------------------------------------

    #[test]
    fn format_hex_empty_bytes() {
        assert_eq!(format_hex(&[]), "0x");
    }

    #[test]
    fn format_hex_has_0x_prefix() {
        let result = format_hex(&[0xab, 0xcd]);
        assert!(result.starts_with("0x"), "output must start with '0x'");
    }

    #[test]
    fn format_hex_known_pattern() {
        assert_eq!(format_hex(&[0xde, 0xad, 0xbe, 0xef]), "0xdeadbeef");
    }

    #[test]
    fn format_hex_is_lowercase() {
        let result = format_hex(&[0xAB, 0xCD]);
        assert_eq!(result, "0xabcd", "hex digits must be lowercase");
    }

    #[test]
    fn format_hex_single_byte() {
        assert_eq!(format_hex(&[0x0f]), "0x0f");
        assert_eq!(format_hex(&[0x00]), "0x00");
        assert_eq!(format_hex(&[0xff]), "0xff");
    }

    // ---------------------------------------------------------------------------
    // format_hex / bytes_from_hex round-trip
    // ---------------------------------------------------------------------------

    #[test]
    fn format_hex_bytes_from_hex_round_trip() {
        let original = vec![0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let hex_str = format_hex(&original);
        let recovered = bytes_from_hex(&hex_str).unwrap();
        assert_eq!(recovered.as_ref(), original.as_slice());
    }

    // ---------------------------------------------------------------------------
    // require_signer_or_dry_run
    // ---------------------------------------------------------------------------

    #[test]
    fn require_signer_or_dry_run_signer_present_no_dry_run() {
        assert!(require_signer_or_dry_run(true, false, "send").is_ok());
    }

    #[test]
    fn require_signer_or_dry_run_dry_run_no_signer() {
        assert!(require_signer_or_dry_run(false, true, "send").is_ok());
    }

    #[test]
    fn require_signer_or_dry_run_both_true() {
        assert!(require_signer_or_dry_run(true, true, "send").is_ok());
    }

    #[test]
    fn require_signer_or_dry_run_neither_errors() {
        let result = require_signer_or_dry_run(false, false, "relay");
        assert!(result.is_err(), "neither signer nor dry-run should produce an error");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("relay"),
            "error message should contain the command name; got: {msg}"
        );
        assert!(
            msg.contains("signer") || msg.contains("dry-run"),
            "error should mention signer or dry-run; got: {msg}"
        );
    }

    #[test]
    fn require_signer_or_dry_run_error_embeds_cmd_name() {
        let cmd = "my-unique-command-xyz";
        let result = require_signer_or_dry_run(false, false, cmd);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains(cmd), "error message must embed the cmd name '{cmd}'");
    }

    // ---------------------------------------------------------------------------
    // u256_to_string / b256_to_hex / address_to_hex
    // ---------------------------------------------------------------------------

    #[test]
    fn u256_to_string_zero() {
        assert_eq!(u256_to_string(U256::ZERO), "0");
    }

    #[test]
    fn u256_to_string_known_value() {
        assert_eq!(u256_to_string(U256::from(42u64)), "42");
    }

    #[test]
    fn b256_to_hex_has_0x_prefix_and_correct_length() {
        let zero = B256::ZERO;
        let result = b256_to_hex(zero);
        assert!(result.starts_with("0x"), "b256_to_hex must start with '0x'");
        // "0x" + 64 hex chars = 66 characters total.
        assert_eq!(result.len(), 66, "b256_to_hex should produce 66 chars for a B256");
    }

    #[test]
    fn b256_to_hex_is_lowercase() {
        let all_ones = B256::repeat_byte(0xff);
        let result = b256_to_hex(all_ones);
        assert_eq!(result, "0x".to_string() + &"ff".repeat(32));
    }

    #[test]
    fn address_to_hex_has_0x_prefix_and_correct_length() {
        let result = address_to_hex(Address::ZERO);
        assert!(result.starts_with("0x"), "address_to_hex must start with '0x'");
        // "0x" + 40 hex chars = 42 characters total.
        assert_eq!(result.len(), 42, "address_to_hex should produce 42 chars for an Address");
    }

    #[test]
    fn address_to_hex_is_lowercase() {
        let all_ones = Address::repeat_byte(0xff);
        let result = address_to_hex(all_ones);
        assert_eq!(result, "0x".to_string() + &"ff".repeat(20));
    }

    // ---------------------------------------------------------------------------
    // parse_b256
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_b256_valid() {
        let hex = "0x".to_string() + &"ab".repeat(32);
        let result = parse_b256(&hex);
        assert!(result.is_ok(), "valid 32-byte hex should parse");
        assert_eq!(result.unwrap(), B256::repeat_byte(0xab));
    }

    #[test]
    fn parse_b256_invalid_too_short() {
        let result = parse_b256("0x1234");
        assert!(result.is_err(), "too-short value should fail");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid bytes32"), "error should mention 'invalid bytes32'");
    }

    #[test]
    fn parse_b256_zero() {
        let hex = "0x".to_string() + &"00".repeat(32);
        let v = parse_b256(&hex).unwrap();
        assert_eq!(v, B256::ZERO);
    }

    // ---------------------------------------------------------------------------
    // Serde round-trips for output structs
    // ---------------------------------------------------------------------------

    #[test]
    fn proof_message_serde_round_trip() {
        let original = ProofMessage {
            tx_number_in_batch: 7,
            sender: "0xdeadbeef".to_string(),
            data: "0x1234".to_string(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: ProofMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered.tx_number_in_batch, 7);
        assert_eq!(recovered.sender, "0xdeadbeef");
        assert_eq!(recovered.data, "0x1234");
    }

    #[test]
    fn proof_message_serde_uses_camel_case_keys() {
        let msg = ProofMessage {
            tx_number_in_batch: 1,
            sender: "s".to_string(),
            data: "d".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("txNumberInBatch"), "key should be camelCase 'txNumberInBatch'");
        assert!(!json.contains("tx_number_in_batch"), "snake_case key must not appear");
    }

    #[test]
    fn message_inclusion_proof_serde_round_trip() {
        let original = MessageInclusionProof {
            chain_id: "271".to_string(),
            l1_batch_number: 42,
            l2_message_index: 3,
            root: "0xaabbcc".to_string(),
            message: ProofMessage {
                tx_number_in_batch: 1,
                sender: "0x1".to_string(),
                data: "0x2".to_string(),
            },
            proof: vec!["0xabc".to_string(), "0xdef".to_string()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: MessageInclusionProof = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.chain_id, "271");
        assert_eq!(recovered.l1_batch_number, 42);
        assert_eq!(recovered.proof.len(), 2);
    }

    #[test]
    fn interop_call_view_serializes_to_camel_case() {
        let view = InteropCallView {
            version: "1".to_string(),
            shadow_account: false,
            to: "0xto".to_string(),
            from: "0xfrom".to_string(),
            value: "0".to_string(),
            data: "0x".to_string(),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("shadowAccount"), "should use camelCase 'shadowAccount'");
        assert!(!json.contains("shadow_account"), "snake_case must not appear");
    }

    #[test]
    fn status_output_serializes_correctly() {
        let output = StatusOutput {
            bundle_hash: "0xhash".to_string(),
            bundle_status: "Executed".to_string(),
            calls: Some(vec![CallStatusView { index: 0, status: "ok".to_string() }]),
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("bundleHash"), "camelCase 'bundleHash' required");
        assert!(json.contains("bundleStatus"), "camelCase 'bundleStatus' required");
        assert!(json.contains("Executed"));
    }
}
