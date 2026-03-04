use crate::types::{address_to_hex, b256_to_hex, format_hex, u256_to_string};
use crate::types::{BundleAttributesView, InteropBundle, InteropBundleView as BundleView};
use crate::types::{InteropCallView, MessageInclusionProof};
use alloy_primitives::ruint::aliases::U256;
use alloy_primitives::{keccak256, Address, Bytes, B256, U256 as AlloyU256};
use alloy_sol_types::{SolCall, SolError, SolValue};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::str::FromStr;

alloy_sol_types::sol! {
    struct InteropBundleSent {
        bytes32 l2l1MsgHash;
        bytes32 interopBundleHash;
        InteropBundle interopBundle;
    }

    struct MessageSentData {
        bytes sender;
        bytes recipient;
        bytes payload;
        uint256 value;
        bytes[] attributes;
    }

    struct L2Message {
        uint16 txNumberInBatch;
        address sender;
        bytes data;
    }

    struct MessageInclusionProofSol {
        uint256 chainId;
        uint256 l1BatchNumber;
        uint256 l2MessageIndex;
        L2Message message;
        bytes32[] proof;
    }

    struct InteropCallStarter {
        bytes to;
        bytes data;
        bytes[] callAttributes;
    }

    function verifyBundle(bytes _bundle, MessageInclusionProofSol _proof);
    function executeBundle(bytes _bundle, MessageInclusionProofSol _proof);
    function sendMessage(bytes recipient, bytes payload, bytes[] attributes) external payable returns (bytes32);
    function sendBundle(bytes _destinationChainId, InteropCallStarter[] _callStarters, bytes[] _bundleAttributes) external payable returns (bytes32);
    function bundleStatus(bytes32 bundleHash) external view returns (uint8);
    function callStatus(bytes32 bundleHash, uint256 callIndex) external view returns (uint8);
    function interopRoots(uint256 chainId, uint256 batchNumber) external view returns (bytes32);

    // 0x9031f751
    error AttributeAlreadySet(bytes4 selector);
    // 0xbcb41ec7
    error AttributeViolatesRestriction(bytes4 selector, uint256 restriction);
    // 0x5bba5111
    error BundleAlreadyProcessed(bytes32 bundleHash);
    // 0xa43d2953
    error BundleVerifiedAlready(bytes32 bundleHash);
    // 0xd5c7a376
    error CallAlreadyExecuted(bytes32 bundleHash, uint256 callIndex);
    // 0xc087b727
    error CallNotExecutable(bytes32 bundleHash, uint256 callIndex);
    // 0xf729f26d
    error CanNotUnbundle(bytes32 bundleHash);
    // 0xe845be4c
    error ExecutingNotAllowed(bytes32 bundleHash, bytes callerAddress, bytes executionAddress);
    // 0x62d214aa
    error IndirectCallValueMismatch(uint256 expected, uint256 actual);
    // 0xfe8b1b16
    error InteroperableAddressChainReferenceNotEmpty(bytes interoperableAddress);
    // 0x884f49ba
    error InteroperableAddressNotEmpty(bytes interoperableAddress);
    // 0xeae192ef
    error InvalidInteropBundleVersion();
    // 0xd5f13973
    error InvalidInteropCallVersion();
    // 0x32c2e156
    error MessageNotIncluded();
    // 0x89fd2c76
    error UnauthorizedMessageSender(address expected, address actual);
    // 0x0345c281
    error UnbundlingNotAllowed(bytes32 bundleHash, bytes callerAddress, bytes unbundlerAddress);
    // 0x801534e9
    error WrongCallStatusLength(uint256 bundleCallsLength, uint256 providedCallStatusLength);
    // 0x4534e972
    error WrongDestinationChainId(bytes32 bundleHash, uint256 expected, uint256 actual);
    // 0x534ab1b2
    error WrongSourceChainId(bytes32 bundleHash, uint256 expected, uint256 actual);

}

pub fn event_topic(signature: &str) -> B256 {
    keccak256(signature.as_bytes())
}

pub fn interop_bundle_sent_topic() -> B256 {
    event_topic(
        "InteropBundleSent(bytes32,bytes32,(bytes1,uint256,uint256,bytes32,(bytes1,bool,address,address,uint256,bytes)[],(bytes,bytes,bool)))",
    )
}

pub fn message_sent_topic() -> B256 {
    event_topic("MessageSent(bytes32,bytes,bytes,bytes,uint256,bytes[])")
}

pub fn l1_message_sent_topic() -> B256 {
    event_topic("L1MessageSent(address,bytes32,bytes)")
}

pub fn bundle_verified_topic() -> B256 {
    event_topic("BundleVerified(bytes32)")
}

pub fn bundle_executed_topic() -> B256 {
    event_topic("BundleExecuted(bytes32)")
}

pub fn bundle_unbundled_topic() -> B256 {
    event_topic("BundleUnbundled(bytes32)")
}

pub fn call_processed_topic() -> B256 {
    event_topic("CallProcessed(bytes32,uint256,uint8)")
}

pub fn decode_interop_bundle_sent(data: Bytes) -> Result<(B256, B256, InteropBundle)> {
    let decoded = InteropBundleSent::abi_decode_params(&data)?;
    Ok((
        decoded.l2l1MsgHash,
        decoded.interopBundleHash,
        decoded.interopBundle,
    ))
}

pub fn decode_message_sent(data: Bytes) -> Result<MessageSentData> {
    Ok(MessageSentData::abi_decode_params(&data)?)
}

pub fn decode_u8(data: Bytes) -> Result<u8> {
    //let value: (u8,) = <(u8,)>::abi_decode(&data)?;
    //Ok(value.0)
    let v: u8 = *data.first().ok_or_else(|| anyhow::anyhow!("empty data"))?;
    Ok(v)
}

// Create a map from every error selector to its name
pub fn error_selector_map() -> HashMap<String, &'static str> {
    let mut map = HashMap::new();
    map.insert(
        hex::encode(AttributeAlreadySet::SELECTOR),
        "AttributeAlreadySet",
    );
    map.insert(
        hex::encode(AttributeViolatesRestriction::SELECTOR),
        "AttributeViolatesRestriction",
    );
    map.insert(
        hex::encode(BundleAlreadyProcessed::SELECTOR),
        "BundleAlreadyProcessed",
    );

    map.insert(
        hex::encode(BundleVerifiedAlready::SELECTOR),
        "BundleVerifiedAlready",
    );
    map.insert(
        hex::encode(CallAlreadyExecuted::SELECTOR),
        "CallAlreadyExecuted",
    );
    map.insert(
        hex::encode(CallNotExecutable::SELECTOR),
        "CallNotExecutable",
    );
    map.insert(hex::encode(CanNotUnbundle::SELECTOR), "CanNotUnbundle");
    map.insert(
        hex::encode(ExecutingNotAllowed::SELECTOR),
        "ExecutingNotAllowed",
    );
    map.insert(
        hex::encode(IndirectCallValueMismatch::SELECTOR),
        "IndirectCallValueMismatch",
    );
    map.insert(
        hex::encode(InteroperableAddressChainReferenceNotEmpty::SELECTOR),
        "InteroperableAddressChainReferenceNotEmpty",
    );
    map.insert(
        hex::encode(InteroperableAddressNotEmpty::SELECTOR),
        "InteroperableAddressNotEmpty",
    );
    map.insert(
        hex::encode(InvalidInteropBundleVersion::SELECTOR),
        "InvalidInteropBundleVersion",
    );
    map.insert(
        hex::encode(InvalidInteropCallVersion::SELECTOR),
        "InvalidInteropCallVersion",
    );
    map.insert(
        hex::encode(MessageNotIncluded::SELECTOR),
        "MessageNotIncluded",
    );
    map.insert(
        hex::encode(UnauthorizedMessageSender::SELECTOR),
        "UnauthorizedMessageSender",
    );
    map.insert(
        hex::encode(UnbundlingNotAllowed::SELECTOR),
        "UnbundlingNotAllowed",
    );
    map.insert(
        hex::encode(WrongCallStatusLength::SELECTOR),
        "WrongCallStatusLength",
    );
    map.insert(
        hex::encode(WrongDestinationChainId::SELECTOR),
        "WrongDestinationChainId",
    );
    map.insert(
        hex::encode(WrongSourceChainId::SELECTOR),
        "WrongSourceChainId",
    );
    map
}

pub fn bundle_view(bundle: &InteropBundle) -> BundleView {
    BundleView {
        version: format_hex(bundle.version.as_ref()),
        source_chain_id: u256_to_string(bundle.sourceChainId),
        destination_chain_id: u256_to_string(bundle.destinationChainId),
        interop_bundle_salt: b256_to_hex(bundle.interopBundleSalt),
        calls: bundle
            .calls
            .iter()
            .map(|call| InteropCallView {
                version: format_hex(call.version.as_ref()),
                shadow_account: call.shadowAccount,
                to: address_to_hex(call.to),
                from: address_to_hex(call.from),
                value: u256_to_string(call.value),
                data: format_hex(call.data.as_ref()),
            })
            .collect(),
        bundle_attributes: BundleAttributesView {
            execution_address: format_hex(bundle.bundleAttributes.executionAddress.as_ref()),
            unbundler_address: format_hex(bundle.bundleAttributes.unbundlerAddress.as_ref()),
            use_fixed_fee: bundle.bundleAttributes.useFixedFee,
        },
    }
}

pub fn encode_interop_bundle(bundle: &InteropBundle) -> Bytes {
    let encoded = bundle.abi_encode();
    Bytes::from(encoded)
}

pub fn encode_verify_bundle_call(
    encoded_bundle: Bytes,
    proof: MessageInclusionProof,
) -> Result<Bytes> {
    let proof = proof_to_sol(proof)?;
    let call = verifyBundleCall {
        _bundle: encoded_bundle,
        _proof: proof,
    };
    Ok(Bytes::from(call.abi_encode()))
}

pub fn encode_execute_bundle_call(
    encoded_bundle: Bytes,
    proof: MessageInclusionProof,
) -> Result<Bytes> {
    let proof = proof_to_sol(proof)?;
    let call = executeBundleCall {
        _bundle: encoded_bundle,
        _proof: proof,
    };
    Ok(Bytes::from(call.abi_encode()))
}

pub fn encode_send_message_call(
    recipient: Bytes,
    payload: Bytes,
    attributes: Vec<Bytes>,
) -> Result<Bytes> {
    let call = sendMessageCall {
        recipient,
        payload,
        attributes,
    };
    Ok(Bytes::from(call.abi_encode()))
}

pub fn encode_send_bundle_call(
    destination_chain: Bytes,
    call_starters: Vec<InteropCallStarter>,
    bundle_attributes: Vec<Bytes>,
) -> Result<Bytes> {
    let call = sendBundleCall {
        _destinationChainId: destination_chain,
        _callStarters: call_starters,
        _bundleAttributes: bundle_attributes,
    };
    Ok(Bytes::from(call.abi_encode()))
}

pub fn encode_bundle_status_call(bundle_hash: B256) -> Bytes {
    let call = bundleStatusCall {
        bundleHash: bundle_hash,
    };
    Bytes::from(call.abi_encode())
}

pub fn encode_call_status_call(bundle_hash: B256, call_index: AlloyU256) -> Bytes {
    let call = callStatusCall {
        bundleHash: bundle_hash,
        callIndex: call_index,
    };
    Bytes::from(call.abi_encode())
}

pub fn encode_interop_roots_call(chain_id: AlloyU256, batch_number: AlloyU256) -> Bytes {
    let call = interopRootsCall {
        chainId: chain_id,
        batchNumber: batch_number,
    };
    Bytes::from(call.abi_encode())
}

fn proof_to_sol(proof: MessageInclusionProof) -> Result<MessageInclusionProofSol> {
    let chain_id = AlloyU256::from_str(&proof.chain_id)
        .map_err(|err| anyhow!("invalid chainId {}: {err}", proof.chain_id))?;
    let sender = Address::from_str(&proof.message.sender)
        .map_err(|err| anyhow!("invalid sender {}: {err}", proof.message.sender))?;
    let data = Bytes::from(hex::decode(proof.message.data.trim_start_matches("0x"))?);
    let proof_nodes = proof
        .proof
        .into_iter()
        .map(|value| B256::from_str(&value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| anyhow!("invalid proof node: {err}"))?;

    Ok(MessageInclusionProofSol {
        chainId: chain_id,
        l1BatchNumber: AlloyU256::from(proof.l1_batch_number),
        l2MessageIndex: AlloyU256::from(proof.l2_message_index),
        message: L2Message {
            txNumberInBatch: proof.message.tx_number_in_batch as u16,
            sender,
            data,
        },
        proof: proof_nodes,
    })
}

pub fn decode_bundle_status(data: Bytes) -> Result<u8> {
    let value: (U256,) = <(U256,)>::abi_decode(&data)?;
    let tmp: u64 = value.0.try_into().unwrap();
    Ok(tmp as u8)
}

pub fn decode_call_status(data: Bytes) -> Result<u8> {
    let value: (U256,) = <(U256,)>::abi_decode(&data)?;
    let tmp: u64 = value.0.try_into().unwrap();
    Ok(tmp as u8)
}

pub fn decode_bytes32(data: Bytes) -> Result<B256> {
    let value: (B256,) = <(B256,)>::abi_decode(&data)?;
    Ok(value.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BundleAttributes, InteropBundle, InteropCall, MessageInclusionProof, ProofMessage};
    use alloy_primitives::{bytes, fixed_bytes, Address, B256, Bytes};
    use alloy_sol_types::SolCall;

    // ---------------------------------------------------------------------------
    // event_topic / known topic hashes
    // ---------------------------------------------------------------------------

    /// The keccak256 of an event signature should be stable and match hand-computed values.
    #[test]
    fn event_topic_known_hash() {
        // keccak256("Transfer(address,address,uint256)") is a widely known value
        let transfer_topic = event_topic("Transfer(address,address,uint256)");
        let expected: B256 =
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                .parse()
                .unwrap();
        assert_eq!(transfer_topic, expected);
    }

    /// event_topic must be deterministic: calling twice with same input gives same result.
    #[test]
    fn event_topic_deterministic() {
        let a = event_topic("Foo(uint256)");
        let b = event_topic("Foo(uint256)");
        assert_eq!(a, b);
    }

    /// Different signatures must produce different topics.
    #[test]
    fn event_topic_different_signatures_differ() {
        let a = event_topic("Foo(uint256)");
        let b = event_topic("Bar(uint256)");
        assert_ne!(a, b);
    }

    // ---------------------------------------------------------------------------
    // Stable topic helper functions
    // ---------------------------------------------------------------------------

    /// All topic helpers must return non-zero hashes (a zero topic would be invalid).
    #[test]
    fn all_topic_helpers_are_nonzero() {
        assert_ne!(interop_bundle_sent_topic(), B256::ZERO);
        assert_ne!(message_sent_topic(), B256::ZERO);
        assert_ne!(l1_message_sent_topic(), B256::ZERO);
        assert_ne!(bundle_verified_topic(), B256::ZERO);
        assert_ne!(bundle_executed_topic(), B256::ZERO);
        assert_ne!(bundle_unbundled_topic(), B256::ZERO);
        assert_ne!(call_processed_topic(), B256::ZERO);
    }

    /// Each topic helper must return a value that matches a direct call to event_topic with the
    /// same signature string.
    #[test]
    fn topic_helpers_match_event_topic() {
        assert_eq!(
            interop_bundle_sent_topic(),
            event_topic("InteropBundleSent(bytes32,bytes32,(bytes1,uint256,uint256,bytes32,(bytes1,bool,address,address,uint256,bytes)[],(bytes,bytes,bool)))")
        );
        assert_eq!(
            message_sent_topic(),
            event_topic("MessageSent(bytes32,bytes,bytes,bytes,uint256,bytes[])")
        );
        assert_eq!(
            l1_message_sent_topic(),
            event_topic("L1MessageSent(address,bytes32,bytes)")
        );
        assert_eq!(
            bundle_verified_topic(),
            event_topic("BundleVerified(bytes32)")
        );
        assert_eq!(
            bundle_executed_topic(),
            event_topic("BundleExecuted(bytes32)")
        );
        assert_eq!(
            bundle_unbundled_topic(),
            event_topic("BundleUnbundled(bytes32)")
        );
        assert_eq!(
            call_processed_topic(),
            event_topic("CallProcessed(bytes32,uint256,uint8)")
        );
    }

    // ---------------------------------------------------------------------------
    // error_selector_map
    // ---------------------------------------------------------------------------

    /// The map must contain all 19 known error selectors.
    #[test]
    fn error_selector_map_has_expected_count() {
        let map = error_selector_map();
        assert_eq!(map.len(), 19, "expected exactly 19 error entries");
    }

    /// The selector values declared in comments must match the keccak-derived selectors that
    /// alloy_sol_types produces from the error signatures.
    #[test]
    fn error_selector_map_known_selectors() {
        let map = error_selector_map();

        // Selectors taken from the comments in the sol! block.
        let cases: &[(&str, &str)] = &[
            ("9031f751", "AttributeAlreadySet"),
            ("bcb41ec7", "AttributeViolatesRestriction"),
            ("5bba5111", "BundleAlreadyProcessed"),
            ("a43d2953", "BundleVerifiedAlready"),
            ("d5c7a376", "CallAlreadyExecuted"),
            ("c087b727", "CallNotExecutable"),
            ("f729f26d", "CanNotUnbundle"),
            ("e845be4c", "ExecutingNotAllowed"),
            ("62d214aa", "IndirectCallValueMismatch"),
            ("fe8b1b16", "InteroperableAddressChainReferenceNotEmpty"),
            ("884f49ba", "InteroperableAddressNotEmpty"),
            ("eae192ef", "InvalidInteropBundleVersion"),
            ("d5f13973", "InvalidInteropCallVersion"),
            ("32c2e156", "MessageNotIncluded"),
            ("89fd2c76", "UnauthorizedMessageSender"),
            ("0345c281", "UnbundlingNotAllowed"),
            ("801534e9", "WrongCallStatusLength"),
            ("4534e972", "WrongDestinationChainId"),
            ("534ab1b2", "WrongSourceChainId"),
        ];

        for (selector_hex, name) in cases {
            let found = map
                .get(*selector_hex)
                .unwrap_or_else(|| panic!("selector {selector_hex} ({name}) not found in map"));
            assert_eq!(*found, *name, "selector {selector_hex} maps to wrong name");
        }
    }

    /// Every value in the map must be a unique error name (no duplicate names).
    #[test]
    fn error_selector_map_values_are_unique() {
        let map = error_selector_map();
        let mut names: Vec<&&str> = map.values().collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate error names found");
    }

    // ---------------------------------------------------------------------------
    // decode_u8
    // ---------------------------------------------------------------------------

    #[test]
    fn decode_u8_first_byte() {
        let data = Bytes::from(vec![0x42, 0x00, 0x00]);
        assert_eq!(decode_u8(data).unwrap(), 0x42);
    }

    #[test]
    fn decode_u8_boundary_zero() {
        let data = Bytes::from(vec![0x00]);
        assert_eq!(decode_u8(data).unwrap(), 0u8);
    }

    #[test]
    fn decode_u8_boundary_max() {
        let data = Bytes::from(vec![0xff]);
        assert_eq!(decode_u8(data).unwrap(), 255u8);
    }

    #[test]
    fn decode_u8_empty_is_error() {
        let result = decode_u8(Bytes::new());
        assert!(result.is_err(), "expected error for empty input");
    }

    // ---------------------------------------------------------------------------
    // decode_bundle_status / decode_call_status
    // ---------------------------------------------------------------------------

    fn abi_encode_u256(value: u64) -> Bytes {
        let v = AlloyU256::from(value);
        Bytes::from(v.abi_encode())
    }

    #[test]
    fn decode_bundle_status_zero() {
        assert_eq!(decode_bundle_status(abi_encode_u256(0)).unwrap(), 0u8);
    }

    #[test]
    fn decode_bundle_status_nonzero() {
        assert_eq!(decode_bundle_status(abi_encode_u256(3)).unwrap(), 3u8);
    }

    #[test]
    fn decode_bundle_status_empty_is_error() {
        assert!(decode_bundle_status(Bytes::new()).is_err());
    }

    #[test]
    fn decode_call_status_round_trip() {
        for v in [0u64, 1, 2, 5, 255] {
            let encoded = abi_encode_u256(v);
            assert_eq!(decode_call_status(encoded).unwrap(), v as u8);
        }
    }

    // ---------------------------------------------------------------------------
    // decode_bytes32
    // ---------------------------------------------------------------------------

    #[test]
    fn decode_bytes32_round_trip() {
        let original: B256 =
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
                .parse()
                .unwrap();
        let encoded = Bytes::from(original.abi_encode());
        assert_eq!(decode_bytes32(encoded).unwrap(), original);
    }

    #[test]
    fn decode_bytes32_zero() {
        let encoded = Bytes::from(B256::ZERO.abi_encode());
        assert_eq!(decode_bytes32(encoded).unwrap(), B256::ZERO);
    }

    #[test]
    fn decode_bytes32_short_input_is_error() {
        // 31 bytes is too short for a full bytes32 ABI word
        let data = Bytes::from(vec![0u8; 31]);
        assert!(decode_bytes32(data).is_err());
    }

    // ---------------------------------------------------------------------------
    // encode_bundle_status_call / encode_call_status_call / encode_interop_roots_call
    // ---------------------------------------------------------------------------

    /// The first 4 bytes of an ABI-encoded function call are the selector. We verify that the
    /// selector embedded in the encoding matches the keccak4 of the function signature.
    #[test]
    fn encode_bundle_status_call_has_correct_selector() {
        let hash: B256 = "0x1122334411223344112233441122334411223344112233441122334411223344"
            .parse()
            .unwrap();
        let encoded = encode_bundle_status_call(hash);
        assert!(encoded.len() >= 4);
        let selector = &encoded[..4];
        assert_eq!(selector, bundleStatusCall::SELECTOR);
    }

    #[test]
    fn encode_bundle_status_call_encodes_hash_argument() {
        let hash: B256 = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
            .parse()
            .unwrap();
        let encoded = encode_bundle_status_call(hash);
        // After the 4-byte selector the hash occupies the first 32-byte word.
        let arg_bytes: [u8; 32] = encoded[4..36].try_into().unwrap();
        assert_eq!(B256::from(arg_bytes), hash);
    }

    #[test]
    fn encode_call_status_call_has_correct_selector() {
        let hash = B256::ZERO;
        let index = AlloyU256::from(0u64);
        let encoded = encode_call_status_call(hash, index);
        assert!(encoded.len() >= 4);
        assert_eq!(&encoded[..4], callStatusCall::SELECTOR);
    }

    #[test]
    fn encode_call_status_call_encodes_both_arguments() {
        let hash: B256 = "0x0101010101010101010101010101010101010101010101010101010101010101"
            .parse()
            .unwrap();
        let index = AlloyU256::from(7u64);
        let encoded = encode_call_status_call(hash, index);
        // selector (4) + hash (32) + index (32)
        assert_eq!(encoded.len(), 68);
        let hash_bytes: [u8; 32] = encoded[4..36].try_into().unwrap();
        assert_eq!(B256::from(hash_bytes), hash);
        let index_bytes: [u8; 32] = encoded[36..68].try_into().unwrap();
        assert_eq!(AlloyU256::from_be_bytes::<32>(index_bytes), index);
    }

    #[test]
    fn encode_interop_roots_call_has_correct_selector() {
        let chain_id = AlloyU256::from(300u64);
        let batch = AlloyU256::from(1u64);
        let encoded = encode_interop_roots_call(chain_id, batch);
        assert!(encoded.len() >= 4);
        assert_eq!(&encoded[..4], interopRootsCall::SELECTOR);
    }

    // ---------------------------------------------------------------------------
    // encode_send_message_call
    // ---------------------------------------------------------------------------

    #[test]
    fn encode_send_message_call_has_correct_selector() {
        let recipient = Bytes::from(vec![0xABu8; 20]);
        let payload = Bytes::from(b"hello".to_vec());
        let attrs: Vec<Bytes> = vec![];
        let encoded = encode_send_message_call(recipient, payload, attrs).unwrap();
        assert!(encoded.len() >= 4);
        assert_eq!(&encoded[..4], sendMessageCall::SELECTOR);
    }

    #[test]
    fn encode_send_message_call_with_attributes() {
        let recipient = Bytes::from(vec![0x01u8; 32]);
        let payload = Bytes::from(b"payload_data".to_vec());
        let attrs: Vec<Bytes> = vec![Bytes::from(vec![0xffu8; 4])];
        let encoded = encode_send_message_call(recipient, payload, attrs).unwrap();
        assert!(encoded.len() > 4);
        assert_eq!(&encoded[..4], sendMessageCall::SELECTOR);
    }

    // ---------------------------------------------------------------------------
    // encode_interop_bundle / encode_verify_bundle_call / encode_execute_bundle_call
    // ---------------------------------------------------------------------------

    fn sample_bundle() -> InteropBundle {
        InteropBundle {
            version: fixed_bytes!("01"),
            sourceChainId: AlloyU256::from(300u64),
            destinationChainId: AlloyU256::from(324u64),
            interopBundleSalt: B256::ZERO,
            calls: vec![],
            bundleAttributes: BundleAttributes {
                executionAddress: Bytes::new(),
                unbundlerAddress: Bytes::new(),
                useFixedFee: false,
            },
        }
    }

    fn sample_proof() -> MessageInclusionProof {
        MessageInclusionProof {
            chain_id: "300".to_string(),
            l1_batch_number: 1,
            l2_message_index: 0,
            root: "0x0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            message: ProofMessage {
                tx_number_in_batch: 0,
                sender: "0x0000000000000000000000000000000000000000".to_string(),
                data: "0x".to_string(),
            },
            proof: vec![],
        }
    }

    #[test]
    fn encode_interop_bundle_returns_nonempty_bytes() {
        let bundle = sample_bundle();
        let encoded = encode_interop_bundle(&bundle);
        assert!(!encoded.is_empty(), "encoded bundle must not be empty");
    }

    #[test]
    fn encode_verify_bundle_call_has_correct_selector() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let proof = sample_proof();
        let call_bytes = encode_verify_bundle_call(encoded_bundle, proof).unwrap();
        assert!(call_bytes.len() >= 4);
        assert_eq!(&call_bytes[..4], verifyBundleCall::SELECTOR);
    }

    #[test]
    fn encode_execute_bundle_call_has_correct_selector() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let proof = sample_proof();
        let call_bytes = encode_execute_bundle_call(encoded_bundle, proof).unwrap();
        assert!(call_bytes.len() >= 4);
        assert_eq!(&call_bytes[..4], executeBundleCall::SELECTOR);
    }

    /// verify and execute bundle calls must have different selectors.
    #[test]
    fn verify_and_execute_selectors_differ() {
        assert_ne!(verifyBundleCall::SELECTOR, executeBundleCall::SELECTOR);
    }

    // ---------------------------------------------------------------------------
    // encode_send_bundle_call
    // ---------------------------------------------------------------------------

    #[test]
    fn encode_send_bundle_call_has_correct_selector() {
        let dest_chain = Bytes::from(vec![0x01, 0x2c]); // 300 in big-endian
        let starters: Vec<InteropCallStarter> = vec![];
        let attrs: Vec<Bytes> = vec![];
        let encoded = encode_send_bundle_call(dest_chain, starters, attrs).unwrap();
        assert!(encoded.len() >= 4);
        assert_eq!(&encoded[..4], sendBundleCall::SELECTOR);
    }

    // ---------------------------------------------------------------------------
    // proof_to_sol (tested indirectly via encode_verify_bundle_call)
    // ---------------------------------------------------------------------------

    #[test]
    fn proof_to_sol_invalid_chain_id_is_error() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let bad_proof = MessageInclusionProof {
            chain_id: "not_a_number".to_string(),
            ..sample_proof()
        };
        assert!(encode_verify_bundle_call(encoded_bundle, bad_proof).is_err());
    }

    #[test]
    fn proof_to_sol_invalid_sender_address_is_error() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let bad_proof = MessageInclusionProof {
            message: ProofMessage {
                sender: "not_an_address".to_string(),
                ..sample_proof().message
            },
            ..sample_proof()
        };
        assert!(encode_verify_bundle_call(encoded_bundle, bad_proof).is_err());
    }

    #[test]
    fn proof_to_sol_invalid_message_data_hex_is_error() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let bad_proof = MessageInclusionProof {
            message: ProofMessage {
                data: "0xZZZZ".to_string(),
                ..sample_proof().message
            },
            ..sample_proof()
        };
        assert!(encode_verify_bundle_call(encoded_bundle, bad_proof).is_err());
    }

    #[test]
    fn proof_to_sol_invalid_proof_node_is_error() {
        let bundle = sample_bundle();
        let encoded_bundle = encode_interop_bundle(&bundle);
        let bad_proof = MessageInclusionProof {
            proof: vec!["not_a_bytes32".to_string()],
            ..sample_proof()
        };
        assert!(encode_verify_bundle_call(encoded_bundle, bad_proof).is_err());
    }

    // ---------------------------------------------------------------------------
    // bundle_view mapping
    // ---------------------------------------------------------------------------

    #[test]
    fn bundle_view_maps_fields_correctly() {
        let bundle = InteropBundle {
            version: fixed_bytes!("01"),
            sourceChainId: AlloyU256::from(300u64),
            destinationChainId: AlloyU256::from(324u64),
            interopBundleSalt: B256::ZERO,
            calls: vec![InteropCall {
                version: fixed_bytes!("01"),
                shadowAccount: true,
                to: "0x000000000000000000000000000000000000abcd"
                    .parse::<Address>()
                    .unwrap(),
                from: "0x000000000000000000000000000000000000dcba"
                    .parse::<Address>()
                    .unwrap(),
                value: AlloyU256::from(42u64),
                data: bytes!("deadbeef"),
            }],
            bundleAttributes: BundleAttributes {
                executionAddress: Bytes::from(vec![0x01u8]),
                unbundlerAddress: Bytes::from(vec![0x02u8]),
                useFixedFee: true,
            },
        };

        let view = bundle_view(&bundle);

        assert_eq!(view.version, "0x01");
        assert_eq!(view.source_chain_id, "300");
        assert_eq!(view.destination_chain_id, "324");
        assert_eq!(view.calls.len(), 1);

        let call_view = &view.calls[0];
        assert_eq!(call_view.version, "0x01");
        assert!(call_view.shadow_account);
        assert_eq!(call_view.value, "42");
        assert_eq!(call_view.data, "0xdeadbeef");

        assert!(view.bundle_attributes.use_fixed_fee);
        assert_eq!(view.bundle_attributes.execution_address, "0x01");
        assert_eq!(view.bundle_attributes.unbundler_address, "0x02");
    }

    #[test]
    fn bundle_view_empty_calls() {
        let bundle = sample_bundle();
        let view = bundle_view(&bundle);
        assert!(view.calls.is_empty());
    }
}
