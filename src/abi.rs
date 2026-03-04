use crate::types::{address_to_hex, b256_to_hex, format_hex, u256_to_string};
use crate::types::{BundleAttributesView, InteropBundle, InteropBundleView as BundleView};
use crate::types::{InteropCallView, MessageInclusionProof};
use alloy_primitives::ruint::aliases::U256;
use alloy_primitives::{keccak256, Address, Bytes, B256, U256 as AlloyU256};
use alloy_sol_types::{SolCall, SolError, SolValue};
use anyhow::{anyhow, Result};
use serde::Serialize;
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

/// Result of offline calldata/revert-data decoding.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedCalldata {
    /// `"function_call"`, `"error"`, `"bundle_struct"`, or `"unknown"`.
    pub kind: String,
    /// Human-readable name (function or error name).
    pub name: String,
    /// Hex-encoded 4-byte selector, or empty for raw structs.
    pub selector: String,
    /// Decoded parameters as a JSON object.
    pub params: serde_json::Value,
}

/// Decode raw hex bytes against all known interop function and error selectors.
///
/// Works entirely offline — no RPC required. Pass calldata from a failed
/// transaction, a bundle file, or any other interop hex blob.
pub fn decode_calldata_bytes(data: &[u8]) -> DecodedCalldata {
    if data.len() < 4 {
        if let Ok(bundle) = InteropBundle::abi_decode(data) {
            return DecodedCalldata {
                kind: "bundle_struct".to_string(),
                name: "InteropBundle".to_string(),
                selector: String::new(),
                params: serde_json::to_value(bundle_view(&bundle)).unwrap_or_default(),
            };
        }
        return DecodedCalldata {
            kind: "unknown".to_string(),
            name: "unknown".to_string(),
            selector: hex::encode(data),
            params: serde_json::Value::Null,
        };
    }

    let sel = &data[..4];
    let sel_hex = hex::encode(sel);
    // SolCall::abi_decode expects params WITHOUT the 4-byte selector prefix.
    let params_data = &data[4..];

    // ── Function calls ──────────────────────────────────────────────────────
    if sel == verifyBundleCall::SELECTOR {
        return match verifyBundleCall::abi_decode(params_data) {
            Ok(c) => fmt_bundle_action_call("verifyBundle", &sel_hex, c._bundle, c._proof),
            Err(e) => fmt_decode_error("function_call", "verifyBundle", &sel_hex, params_data, &e),
        };
    }
    if sel == executeBundleCall::SELECTOR {
        return match executeBundleCall::abi_decode(params_data) {
            Ok(c) => fmt_bundle_action_call("executeBundle", &sel_hex, c._bundle, c._proof),
            Err(e) => fmt_decode_error("function_call", "executeBundle", &sel_hex, params_data, &e),
        };
    }
    if sel == sendMessageCall::SELECTOR {
        return match sendMessageCall::abi_decode(params_data) {
            Ok(c) => DecodedCalldata {
                kind: "function_call".to_string(),
                name: "sendMessage".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "recipient": format!("0x{}", hex::encode(&c.recipient)),
                    "payload":   format!("0x{}", hex::encode(&c.payload)),
                    "attributes": c.attributes.iter()
                        .map(|a| format!("0x{}", hex::encode(a)))
                        .collect::<Vec<_>>(),
                }),
            },
            Err(e) => fmt_decode_error("function_call", "sendMessage", &sel_hex, params_data, &e),
        };
    }
    if sel == sendBundleCall::SELECTOR {
        return match sendBundleCall::abi_decode(params_data) {
            Ok(c) => {
                let starters: Vec<serde_json::Value> = c
                    ._callStarters
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "to":   format!("0x{}", hex::encode(&s.to)),
                            "data": format!("0x{}", hex::encode(&s.data)),
                            "attributes": s.callAttributes.iter()
                                .map(|a| format!("0x{}", hex::encode(a)))
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                DecodedCalldata {
                    kind: "function_call".to_string(),
                    name: "sendBundle".to_string(),
                    selector: sel_hex,
                    params: serde_json::json!({
                        "destinationChainId": format!("0x{}", hex::encode(&c._destinationChainId)),
                        "callStarters": starters,
                        "bundleAttributes": c._bundleAttributes.iter()
                            .map(|a| format!("0x{}", hex::encode(a)))
                            .collect::<Vec<_>>(),
                    }),
                }
            }
            Err(e) => fmt_decode_error("function_call", "sendBundle", &sel_hex, params_data, &e),
        };
    }
    if sel == bundleStatusCall::SELECTOR {
        return match bundleStatusCall::abi_decode(params_data) {
            Ok(c) => DecodedCalldata {
                kind: "function_call".to_string(),
                name: "bundleStatus".to_string(),
                selector: sel_hex,
                params: serde_json::json!({ "bundleHash": format!("{:#x}", c.bundleHash) }),
            },
            Err(e) => fmt_decode_error("function_call", "bundleStatus", &sel_hex, params_data, &e),
        };
    }
    if sel == callStatusCall::SELECTOR {
        return match callStatusCall::abi_decode(params_data) {
            Ok(c) => DecodedCalldata {
                kind: "function_call".to_string(),
                name: "callStatus".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash": format!("{:#x}", c.bundleHash),
                    "callIndex":  c.callIndex.to_string(),
                }),
            },
            Err(e) => fmt_decode_error("function_call", "callStatus", &sel_hex, params_data, &e),
        };
    }
    if sel == interopRootsCall::SELECTOR {
        return match interopRootsCall::abi_decode(params_data) {
            Ok(c) => DecodedCalldata {
                kind: "function_call".to_string(),
                name: "interopRoots".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "chainId":     c.chainId.to_string(),
                    "batchNumber": c.batchNumber.to_string(),
                }),
            },
            Err(e) => fmt_decode_error("function_call", "interopRoots", &sel_hex, params_data, &e),
        };
    }

    // ── Error selectors (try full decode, fall back to raw params) ───────────
    if sel == AttributeAlreadySet::SELECTOR {
        return match AttributeAlreadySet::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "AttributeAlreadySet".to_string(),
                selector: sel_hex,
                params: serde_json::json!({ "selector": format!("0x{}", hex::encode(e.selector.as_slice())) }),
            },
            Err(_) => fmt_raw_error("AttributeAlreadySet", &sel_hex, params_data),
        };
    }
    if sel == AttributeViolatesRestriction::SELECTOR {
        return match AttributeViolatesRestriction::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "AttributeViolatesRestriction".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "selector":    format!("0x{}", hex::encode(e.selector.as_slice())),
                    "restriction": e.restriction.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("AttributeViolatesRestriction", &sel_hex, params_data),
        };
    }
    if sel == BundleAlreadyProcessed::SELECTOR {
        return match BundleAlreadyProcessed::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "BundleAlreadyProcessed".to_string(),
                selector: sel_hex,
                params: serde_json::json!({ "bundleHash": format!("{:#x}", e.bundleHash) }),
            },
            Err(_) => fmt_raw_error("BundleAlreadyProcessed", &sel_hex, params_data),
        };
    }
    if sel == BundleVerifiedAlready::SELECTOR {
        return match BundleVerifiedAlready::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "BundleVerifiedAlready".to_string(),
                selector: sel_hex,
                params: serde_json::json!({ "bundleHash": format!("{:#x}", e.bundleHash) }),
            },
            Err(_) => fmt_raw_error("BundleVerifiedAlready", &sel_hex, params_data),
        };
    }
    if sel == CallAlreadyExecuted::SELECTOR {
        return match CallAlreadyExecuted::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "CallAlreadyExecuted".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash": format!("{:#x}", e.bundleHash),
                    "callIndex":  e.callIndex.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("CallAlreadyExecuted", &sel_hex, params_data),
        };
    }
    if sel == CallNotExecutable::SELECTOR {
        return match CallNotExecutable::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "CallNotExecutable".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash": format!("{:#x}", e.bundleHash),
                    "callIndex":  e.callIndex.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("CallNotExecutable", &sel_hex, params_data),
        };
    }
    if sel == CanNotUnbundle::SELECTOR {
        return match CanNotUnbundle::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "CanNotUnbundle".to_string(),
                selector: sel_hex,
                params: serde_json::json!({ "bundleHash": format!("{:#x}", e.bundleHash) }),
            },
            Err(_) => fmt_raw_error("CanNotUnbundle", &sel_hex, params_data),
        };
    }
    if sel == ExecutingNotAllowed::SELECTOR {
        return match ExecutingNotAllowed::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "ExecutingNotAllowed".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash":       format!("{:#x}", e.bundleHash),
                    "callerAddress":    format!("0x{}", hex::encode(&e.callerAddress)),
                    "executionAddress": format!("0x{}", hex::encode(&e.executionAddress)),
                }),
            },
            Err(_) => fmt_raw_error("ExecutingNotAllowed", &sel_hex, params_data),
        };
    }
    if sel == IndirectCallValueMismatch::SELECTOR {
        return match IndirectCallValueMismatch::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "IndirectCallValueMismatch".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "expected": e.expected.to_string(),
                    "actual":   e.actual.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("IndirectCallValueMismatch", &sel_hex, params_data),
        };
    }
    if sel == InvalidInteropBundleVersion::SELECTOR {
        return DecodedCalldata {
            kind: "error".to_string(),
            name: "InvalidInteropBundleVersion".to_string(),
            selector: sel_hex,
            params: serde_json::Value::Object(Default::default()),
        };
    }
    if sel == InvalidInteropCallVersion::SELECTOR {
        return DecodedCalldata {
            kind: "error".to_string(),
            name: "InvalidInteropCallVersion".to_string(),
            selector: sel_hex,
            params: serde_json::Value::Object(Default::default()),
        };
    }
    if sel == MessageNotIncluded::SELECTOR {
        return DecodedCalldata {
            kind: "error".to_string(),
            name: "MessageNotIncluded".to_string(),
            selector: sel_hex,
            params: serde_json::Value::Object(Default::default()),
        };
    }
    if sel == UnauthorizedMessageSender::SELECTOR {
        return match UnauthorizedMessageSender::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "UnauthorizedMessageSender".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "expected": format!("{:#x}", e.expected),
                    "actual":   format!("{:#x}", e.actual),
                }),
            },
            Err(_) => fmt_raw_error("UnauthorizedMessageSender", &sel_hex, params_data),
        };
    }
    if sel == UnbundlingNotAllowed::SELECTOR {
        return match UnbundlingNotAllowed::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "UnbundlingNotAllowed".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash":       format!("{:#x}", e.bundleHash),
                    "callerAddress":    format!("0x{}", hex::encode(&e.callerAddress)),
                    "unbundlerAddress": format!("0x{}", hex::encode(&e.unbundlerAddress)),
                }),
            },
            Err(_) => fmt_raw_error("UnbundlingNotAllowed", &sel_hex, params_data),
        };
    }
    if sel == WrongCallStatusLength::SELECTOR {
        return match WrongCallStatusLength::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "WrongCallStatusLength".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleCallsLength":        e.bundleCallsLength.to_string(),
                    "providedCallStatusLength": e.providedCallStatusLength.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("WrongCallStatusLength", &sel_hex, params_data),
        };
    }
    if sel == WrongDestinationChainId::SELECTOR {
        return match WrongDestinationChainId::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "WrongDestinationChainId".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash": format!("{:#x}", e.bundleHash),
                    "expected":   e.expected.to_string(),
                    "actual":     e.actual.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("WrongDestinationChainId", &sel_hex, params_data),
        };
    }
    if sel == WrongSourceChainId::SELECTOR {
        return match WrongSourceChainId::abi_decode(data) {
            Ok(e) => DecodedCalldata {
                kind: "error".to_string(),
                name: "WrongSourceChainId".to_string(),
                selector: sel_hex,
                params: serde_json::json!({
                    "bundleHash": format!("{:#x}", e.bundleHash),
                    "expected":   e.expected.to_string(),
                    "actual":     e.actual.to_string(),
                }),
            },
            Err(_) => fmt_raw_error("WrongSourceChainId", &sel_hex, params_data),
        };
    }

    // ── Fallback: try as raw InteropBundle struct (e.g. contents of a .hex file) ─
    if let Ok(bundle) = InteropBundle::abi_decode(data) {
        return DecodedCalldata {
            kind: "bundle_struct".to_string(),
            name: "InteropBundle".to_string(),
            selector: String::new(),
            params: serde_json::to_value(bundle_view(&bundle)).unwrap_or_default(),
        };
    }

    DecodedCalldata {
        kind: "unknown".to_string(),
        name: "unknown".to_string(),
        selector: sel_hex,
        params: serde_json::json!({ "raw": format!("0x{}", hex::encode(data)) }),
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

fn fmt_bundle_action_call(
    name: &str,
    sel_hex: &str,
    bundle_bytes: Bytes,
    proof: MessageInclusionProofSol,
) -> DecodedCalldata {
    let bundle_hex = format!("0x{}", hex::encode(&bundle_bytes));
    let bundle_decoded = InteropBundle::abi_decode(&bundle_bytes)
        .ok()
        .map(|b| serde_json::to_value(bundle_view(&b)).unwrap_or_default());
    DecodedCalldata {
        kind: "function_call".to_string(),
        name: name.to_string(),
        selector: sel_hex.to_string(),
        params: serde_json::json!({
            "bundleHex": bundle_hex,
            "bundle": bundle_decoded,
            "proof": {
                "chainId":          proof.chainId.to_string(),
                "l1BatchNumber":    proof.l1BatchNumber.to_string(),
                "l2MessageIndex":   proof.l2MessageIndex.to_string(),
                "message": {
                    "txNumberInBatch": proof.message.txNumberInBatch,
                    "sender":          format!("{:#x}", proof.message.sender),
                    "data":            format!("0x{}", hex::encode(&proof.message.data)),
                },
                "proofNodes": proof.proof.iter().map(|p| format!("{p:#x}")).collect::<Vec<_>>(),
            },
        }),
    }
}

fn fmt_decode_error(
    kind: &str,
    name: &str,
    sel_hex: &str,
    params_data: &[u8],
    err: &dyn std::fmt::Display,
) -> DecodedCalldata {
    DecodedCalldata {
        kind: kind.to_string(),
        name: name.to_string(),
        selector: sel_hex.to_string(),
        params: serde_json::json!({
            "decodeError": err.to_string(),
            "rawParams": format!("0x{}", hex::encode(params_data)),
        }),
    }
}

fn fmt_raw_error(name: &str, sel_hex: &str, params_data: &[u8]) -> DecodedCalldata {
    DecodedCalldata {
        kind: "error".to_string(),
        name: name.to_string(),
        selector: sel_hex.to_string(),
        params: serde_json::json!({ "rawParams": format!("0x{}", hex::encode(params_data)) }),
    }
}
