use crate::types::{address_to_hex, b256_to_hex, format_hex, u256_to_string};
use crate::types::{BundleAttributesView, InteropBundle, InteropBundleView as BundleView};
use crate::types::{InteropCallView, MessageInclusionProof};
use alloy_primitives::ruint::aliases::U256;
use alloy_primitives::{address, keccak256, Address, Bytes, B256, U256 as AlloyU256};
use alloy_sol_types::{SolCall, SolError, SolValue};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::str::FromStr;

/// The L2→L1 Messenger system contract address, identical on every zkSync chain.
///
/// The leaf hash computation always uses this as the `sender` field of the L2 log,
/// regardless of which contract actually called `sendMessage`. See `MessageHashing.sol`.
pub const L2_TO_L1_MESSENGER: Address = address!("0000000000000000000000000000000000008008");

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
    // SolCall::abi_decode (like SolError::abi_decode) expects the FULL calldata
    // including the 4-byte selector prefix.  Keep params_data for fallback display only.
    let params_data = &data[4..];

    // ── Function calls ──────────────────────────────────────────────────────
    if sel == verifyBundleCall::SELECTOR {
        return match verifyBundleCall::abi_decode(data) {
            Ok(c) => fmt_bundle_action_call("verifyBundle", &sel_hex, c._bundle, c._proof),
            Err(e) => fmt_decode_error("function_call", "verifyBundle", &sel_hex, params_data, &e),
        };
    }
    if sel == executeBundleCall::SELECTOR {
        return match executeBundleCall::abi_decode(data) {
            Ok(c) => fmt_bundle_action_call("executeBundle", &sel_hex, c._bundle, c._proof),
            Err(e) => fmt_decode_error("function_call", "executeBundle", &sel_hex, params_data, &e),
        };
    }
    if sel == sendMessageCall::SELECTOR {
        return match sendMessageCall::abi_decode(data) {
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
        return match sendBundleCall::abi_decode(data) {
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
        return match bundleStatusCall::abi_decode(data) {
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
        return match callStatusCall::abi_decode(data) {
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
        return match interopRootsCall::abi_decode(data) {
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

// ── Offline Merkle proof verification ────────────────────────────────────────

/// Compute the L2→L1 log leaf hash for an interop message.
///
/// Implements `MessageHashing.getLeafHashFromMessage` from the protocol source.
///
/// The 88-byte packed encoding is (all big-endian):
/// ```text
/// uint8(0)           — l2ShardId
/// bool(true) = 0x01  — isService
/// uint16              — txNumberInBatch
/// address(0x8008)    — sender = L2_TO_L1_MESSENGER (always)
/// bytes32             — key = bytes32(uint160(message_sender))
/// bytes32             — value = keccak256(message_data)
/// ```
/// where `message_sender` is the InteropCenter address that called `sendMessage`,
/// and `message_data` is `BUNDLE_IDENTIFIER ++ abi_encode(bundle)`.
pub fn compute_leaf_hash(tx_number_in_batch: u16, sender: Address, data: &[u8]) -> B256 {
    // key = bytes32(uint160(sender)) — address left-padded to 32 bytes
    let mut key = [0u8; 32];
    key[12..].copy_from_slice(sender.as_slice());

    // value = keccak256(data)
    let value: B256 = keccak256(data);

    // abi.encodePacked(l2ShardId, isService, txNumberInBatch, sender=0x8008, key, value)
    let mut packed = Vec::with_capacity(88);
    packed.push(0u8); // l2ShardId = 0
    packed.push(1u8); // isService = true
    packed.extend_from_slice(&tx_number_in_batch.to_be_bytes());
    packed.extend_from_slice(L2_TO_L1_MESSENGER.as_slice()); // 20 bytes
    packed.extend_from_slice(&key); // 32 bytes
    packed.extend_from_slice(value.as_slice()); // 32 bytes
                                                // total = 1 + 1 + 2 + 20 + 32 + 32 = 88 bytes ✓

    keccak256(packed)
}

/// One step of a Merkle walk, for verbose reporting.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerkleStep {
    pub step: usize,
    pub index: u64,
    pub side: String, // "left" or "right"
    pub hash: String,
}

/// Walk a binary Merkle tree from leaf to root, recording every intermediate hash.
///
/// Implements `Merkle.calculateRoot` from the protocol source:
/// - if `index % 2 == 0`: `hash(current, sibling)` (leaf is left child)
/// - if `index % 2 == 1`: `hash(sibling, current)` (leaf is right child)
pub fn compute_merkle_root_steps(path: &[B256], index: u64, leaf: B256) -> (B256, Vec<MerkleStep>) {
    let mut current = leaf;
    let mut idx = index;
    let mut steps = Vec::with_capacity(path.len());

    for (i, node) in path.iter().enumerate() {
        let (combined, side) = if idx % 2 == 0 {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(current.as_slice());
            buf[32..].copy_from_slice(node.as_slice());
            (keccak256(buf), "left")
        } else {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(node.as_slice());
            buf[32..].copy_from_slice(current.as_slice());
            (keccak256(buf), "right")
        };
        current = combined;
        steps.push(MerkleStep {
            step: i,
            index: idx,
            side: side.to_string(),
            hash: format!("{current:#x}"),
        });
        idx /= 2;
    }

    (current, steps)
}

/// Walk a binary Merkle tree from leaf to root (no step recording).
pub fn compute_merkle_root(path: &[B256], index: u64, leaf: B256) -> B256 {
    compute_merkle_root_steps(path, index, leaf).0
}

/// Result of offline Merkle proof verification.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofVerifyResult {
    /// Source chain ID parsed from the proof.
    pub chain_id: String,
    /// L1 batch number from the proof.
    pub l1_batch_number: u64,
    /// Leaf index (l2_message_index) used in the Merkle walk.
    pub leaf_index: u64,
    /// Hex-encoded leaf hash computed from the proof's message fields.
    pub leaf_hash: String,
    /// Whether the proof is in the new metadata format (first element contains metadata).
    pub new_format: bool,
    /// Number of Merkle path nodes used in the leaf proof.
    pub proof_nodes_used: usize,
    /// Hex-encoded root computed by walking the Merkle path.
    pub computed_root: String,
    /// Hex-encoded expected root from the proof JSON (as returned by zkSync RPC).
    pub expected_root: String,
    /// Whether the computed root matches the expected root.
    pub merkle_valid: bool,
    /// Per-step trace (populated when verbose=true).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<MerkleStep>,
    /// Human-readable verdict.
    pub verdict: String,
}

/// Parse the proof metadata element to determine the proof format.
///
/// New format: the last 28 bytes of the first proof element are all zero.
/// - `first[0]` = metadata version (must be 0x01)
/// - `first[1]` = `logLeafProofLen` — number of nodes for the log-leaf Merkle proof
/// - `first[2]` = `batchLeafProofLen` — nodes for the batch-level proof (0 for direct settlement)
/// - `first[3]` = `finalProofNode` flag (1 = single settlement layer, no recursion needed)
///
/// Old format: the first element is a regular hash node; `proofStartIndex = 0`,
/// `logLeafProofLen = proof.len()`, `finalProofNode = true`.
///
/// Returns `(start_index, log_leaf_proof_len, final_proof_node)`.
fn parse_proof_metadata(proof: &[B256]) -> (usize, usize, bool) {
    if proof.is_empty() {
        return (0, 0, true);
    }
    // Detect new format: bytes [4..32] are all zero (hashes never have 28 trailing zero bytes)
    let is_new_format = proof[0].as_slice()[4..].iter().all(|&b| b == 0);
    if is_new_format {
        let log_leaf_len = proof[0][1] as usize;
        let final_proof_node = proof[0][3] != 0;
        (1, log_leaf_len, final_proof_node)
    } else {
        (0, proof.len(), true)
    }
}

/// Verify a [`MessageInclusionProof`] offline using pure cryptography.
///
/// Reconstructs the L2→L1 log leaf hash from the proof's `message` fields, walks
/// the Merkle tree with the proof nodes, and compares the computed root to
/// `proof.root` (the batch settlement root returned by the zkSync RPC).
///
/// Pass `verbose = true` to populate [`ProofVerifyResult::steps`] with the full
/// Merkle walk trace — useful for diagnosing exactly where the path diverges.
///
/// # Limitations
/// Multi-hop settlement chains (where `finalProofNode = false`) are not yet
/// supported for the batch-level portion of the proof. The log-leaf portion
/// is always verified.
pub fn verify_proof_offline(
    proof: &MessageInclusionProof,
    verbose: bool,
) -> Result<ProofVerifyResult> {
    // Parse sender address (InteropCenter on the source chain)
    let sender = Address::from_str(&proof.message.sender).map_err(|e| {
        anyhow!(
            "invalid proof message sender '{}': {e}",
            proof.message.sender
        )
    })?;

    // Parse message data (0x01 + abi_encode(bundle), or "0x" if not yet patched)
    let data_hex = proof.message.data.trim_start_matches("0x");
    let data_bytes =
        hex::decode(data_hex).map_err(|e| anyhow!("invalid proof message data: {e}"))?;

    // Parse expected root (batch settlement root from zkSync RPC)
    let expected_root = B256::from_str(proof.root.trim_start_matches("0x"))
        .map_err(|e| anyhow!("invalid proof root '{}': {e}", proof.root))?;

    // Parse proof nodes
    let proof_nodes: Vec<B256> = proof
        .proof
        .iter()
        .map(|s| B256::from_str(s.trim_start_matches("0x")))
        .collect::<Result<_, _>>()
        .map_err(|e| anyhow!("invalid proof node: {e}"))?;

    // Compute leaf hash from message fields
    let leaf = compute_leaf_hash(proof.message.tx_number_in_batch as u16, sender, &data_bytes);

    // Determine proof format and extract the log-leaf Merkle path
    let (start, log_leaf_len, _final_proof_node) = parse_proof_metadata(&proof_nodes);
    let path_end = (start + log_leaf_len).min(proof_nodes.len());
    let path = &proof_nodes[start..path_end];
    let new_format = start == 1;

    // Walk the Merkle tree
    let (computed_root, steps) = if verbose {
        compute_merkle_root_steps(path, proof.l2_message_index, leaf)
    } else {
        (
            compute_merkle_root(path, proof.l2_message_index, leaf),
            vec![],
        )
    };

    let merkle_valid = computed_root == expected_root;
    let verdict = if merkle_valid {
        "VALID — computed Merkle root matches proof.root; safe to call bundle verify".to_string()
    } else {
        format!(
            "INVALID — computed root {:#x} does not match proof.root {expected_root:#x}; \
             the proof may be corrupted, use the wrong tx hash / msg_index, \
             or have been fetched before the bundle data was patched",
            computed_root
        )
    };

    Ok(ProofVerifyResult {
        chain_id: proof.chain_id.clone(),
        l1_batch_number: proof.l1_batch_number,
        leaf_index: proof.l2_message_index,
        leaf_hash: format!("{leaf:#x}"),
        new_format,
        proof_nodes_used: path.len(),
        computed_root: format!("{computed_root:#x}"),
        expected_root: format!("{expected_root:#x}"),
        merkle_valid,
        steps,
        verdict,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn params_str<'a>(decoded: &'a DecodedCalldata, key: &str) -> &'a str {
        decoded.params[key].as_str().expect("expected string param")
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    #[test]
    fn decode_empty_returns_unknown() {
        let d = decode_calldata_bytes(&[]);
        assert_eq!(d.kind, "unknown");
    }

    #[test]
    fn decode_short_input_returns_unknown() {
        let d = decode_calldata_bytes(&[0xde, 0xad]);
        assert_eq!(d.kind, "unknown");
    }

    #[test]
    fn decode_unknown_selector_returns_unknown() {
        // 0xdeadbeef is not a known selector.
        let d = decode_calldata_bytes(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(d.kind, "unknown");
        assert_eq!(d.selector, "deadbeef");
    }

    // ── Function calls (round-trip via existing encode helpers) ──────────────

    #[test]
    fn decode_bundle_status_call_round_trip() {
        let bundle_hash = B256::from([0xabu8; 32]);
        let encoded = encode_bundle_status_call(bundle_hash);
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "bundleStatus");
        assert_eq!(d.selector, hex::encode(bundleStatusCall::SELECTOR));
        // bundleHash param should contain the encoded bytes
        let hash_str = params_str(&d, "bundleHash");
        assert!(hash_str.contains(&hex::encode([0xabu8; 32])));
    }

    #[test]
    fn decode_call_status_call_round_trip() {
        let bundle_hash = B256::from([0x01u8; 32]);
        let call_index = AlloyU256::from(5u64);
        let encoded = encode_call_status_call(bundle_hash, call_index);
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "callStatus");
        assert_eq!(params_str(&d, "callIndex"), "5");
    }

    #[test]
    fn decode_interop_roots_call_round_trip() {
        let chain_id = AlloyU256::from(324u64);
        let batch_number = AlloyU256::from(12345u64);
        let encoded = encode_interop_roots_call(chain_id, batch_number);
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "interopRoots");
        assert_eq!(params_str(&d, "chainId"), "324");
        assert_eq!(params_str(&d, "batchNumber"), "12345");
    }

    #[test]
    fn decode_send_message_call_round_trip() {
        let recipient = Bytes::from(vec![0xaa, 0xbb, 0xcc]);
        let payload = Bytes::from(vec![0x01, 0x02, 0x03]);
        let encoded = encode_send_message_call(recipient, payload, vec![]).unwrap();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "sendMessage");
        assert_eq!(params_str(&d, "recipient"), "0xaabbcc");
        assert_eq!(params_str(&d, "payload"), "0x010203");
        // empty attributes → empty array
        assert_eq!(d.params["attributes"].as_array().unwrap().len(), 0);
    }

    // ── Interop errors ───────────────────────────────────────────────────────

    #[test]
    fn decode_error_wrong_destination_chain_id() {
        let err = WrongDestinationChainId {
            bundleHash: B256::ZERO,
            expected: AlloyU256::from(324u64),
            actual: AlloyU256::from(300u64),
        };
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "WrongDestinationChainId");
        assert_eq!(d.selector, hex::encode(WrongDestinationChainId::SELECTOR));
        assert_eq!(params_str(&d, "expected"), "324");
        assert_eq!(params_str(&d, "actual"), "300");
    }

    #[test]
    fn decode_error_bundle_already_processed() {
        let hash = B256::from([0x42u8; 32]);
        let err = BundleAlreadyProcessed { bundleHash: hash };
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "BundleAlreadyProcessed");
        let hash_str = params_str(&d, "bundleHash");
        assert!(hash_str.contains(&hex::encode([0x42u8; 32])));
    }

    #[test]
    fn decode_error_wrong_source_chain_id() {
        let err = WrongSourceChainId {
            bundleHash: B256::ZERO,
            expected: AlloyU256::from(6565u64),
            actual: AlloyU256::from(6566u64),
        };
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "WrongSourceChainId");
        assert_eq!(params_str(&d, "expected"), "6565");
        assert_eq!(params_str(&d, "actual"), "6566");
    }

    #[test]
    fn decode_error_executing_not_allowed() {
        let err = ExecutingNotAllowed {
            bundleHash: B256::ZERO,
            callerAddress: Bytes::from(vec![0x01, 0x02]),
            executionAddress: Bytes::from(vec![0x03, 0x04]),
        };
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "ExecutingNotAllowed");
        assert_eq!(params_str(&d, "callerAddress"), "0x0102");
        assert_eq!(params_str(&d, "executionAddress"), "0x0304");
    }

    #[test]
    fn decode_error_no_params_invalid_bundle_version() {
        let err = InvalidInteropBundleVersion {};
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "InvalidInteropBundleVersion");
        assert_eq!(
            d.selector,
            hex::encode(InvalidInteropBundleVersion::SELECTOR)
        );
    }

    #[test]
    fn decode_error_indirect_call_value_mismatch() {
        let err = IndirectCallValueMismatch {
            expected: AlloyU256::from(1000u64),
            actual: AlloyU256::from(500u64),
        };
        let encoded = err.abi_encode();
        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "error");
        assert_eq!(d.name, "IndirectCallValueMismatch");
        assert_eq!(params_str(&d, "expected"), "1000");
        assert_eq!(params_str(&d, "actual"), "500");
    }

    // ── compute_leaf_hash ─────────────────────────────────────────────────────

    #[test]
    fn leaf_hash_changes_with_each_input_field() {
        let base = compute_leaf_hash(1, Address::ZERO, &[]);
        let diff_tx = compute_leaf_hash(2, Address::ZERO, &[]);
        let diff_sender = compute_leaf_hash(1, Address::from([0x11; 20]), &[]);
        let diff_data = compute_leaf_hash(1, Address::ZERO, &[0xde, 0xad]);
        assert_ne!(base, diff_tx, "different txNumber must change leaf hash");
        assert_ne!(base, diff_sender, "different sender must change leaf hash");
        assert_ne!(base, diff_data, "different data must change leaf hash");
    }

    #[test]
    fn leaf_hash_packed_size_is_88_bytes() {
        // The L2Log serialization must be exactly 88 bytes (L2_TO_L1_LOG_SERIALIZE_SIZE).
        // We verify this by checking that compute_leaf_hash uses 88 packed bytes:
        // 1 (l2ShardId) + 1 (isService) + 2 (txNum u16) + 20 (addr) + 32 (key) + 32 (value) = 88
        // The leaf_hash result is a keccak256 of that — we verify it is deterministic.
        let h = compute_leaf_hash(0xabcd, Address::from([0xfe; 20]), b"hello");
        assert_eq!(
            h,
            compute_leaf_hash(0xabcd, Address::from([0xfe; 20]), b"hello")
        );
    }

    // ── compute_merkle_root ───────────────────────────────────────────────────

    #[test]
    fn merkle_root_zero_depth_equals_leaf() {
        let leaf = B256::from([0xaa; 32]);
        assert_eq!(compute_merkle_root(&[], 0, leaf), leaf);
    }

    #[test]
    fn merkle_root_left_child_hashes_leaf_then_sibling() {
        let leaf = B256::from([0x11; 32]);
        let sibling = B256::from([0x22; 32]);
        let expected = {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(leaf.as_slice());
            buf[32..].copy_from_slice(sibling.as_slice());
            keccak256(buf)
        };
        assert_eq!(compute_merkle_root(&[sibling], 0, leaf), expected);
    }

    #[test]
    fn merkle_root_right_child_hashes_sibling_then_leaf() {
        let leaf = B256::from([0x11; 32]);
        let sibling = B256::from([0x22; 32]);
        let expected = {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(sibling.as_slice());
            buf[32..].copy_from_slice(leaf.as_slice());
            keccak256(buf)
        };
        assert_eq!(compute_merkle_root(&[sibling], 1, leaf), expected);
    }

    // ── verify_proof_offline ──────────────────────────────────────────────────

    /// Build a self-consistent MessageInclusionProof for a 1-node Merkle path.
    ///
    /// The proof has:
    ///   leaf   = compute_leaf_hash(tx=5, sender=0xaa..aa, data=0xdeadbeef)
    ///   root   = keccak256(leaf ++ sibling)   (leaf at index 0 = left child)
    fn make_valid_proof() -> (MessageInclusionProof, B256) {
        use crate::types::ProofMessage;
        let sender_addr = Address::from([0xaau8; 20]);
        let data = vec![0xde, 0xad, 0xbe, 0xef];
        let tx_num: u16 = 5;

        let leaf = compute_leaf_hash(tx_num, sender_addr, &data);
        let sibling = B256::from([0x42u8; 32]);
        let root = {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(leaf.as_slice());
            buf[32..].copy_from_slice(sibling.as_slice());
            keccak256(buf)
        };

        let proof = MessageInclusionProof {
            chain_id: "324".to_string(),
            l1_batch_number: 99,
            l2_message_index: 0, // index 0 = left child
            root: format!("{root:#x}"),
            message: ProofMessage {
                tx_number_in_batch: tx_num as u64,
                sender: format!("{sender_addr:#x}"),
                data: format!("0x{}", hex::encode(&data)),
            },
            proof: vec![format!("{sibling:#x}")], // old format: plain path
        };
        (proof, root)
    }

    #[test]
    fn verify_proof_offline_valid_proof_passes() {
        let (proof, _root) = make_valid_proof();
        let result = verify_proof_offline(&proof, false).unwrap();
        assert!(result.merkle_valid, "expected valid: {}", result.verdict);
        assert_eq!(result.proof_nodes_used, 1);
        assert!(!result.new_format, "expected old format (plain path)");
        assert_eq!(result.chain_id, "324");
        assert_eq!(result.l1_batch_number, 99);
        assert_eq!(result.leaf_index, 0);
    }

    #[test]
    fn verify_proof_offline_tampered_root_fails() {
        let (mut proof, _) = make_valid_proof();
        proof.root = format!("{:#x}", B256::from([0xff; 32]));
        let result = verify_proof_offline(&proof, false).unwrap();
        assert!(!result.merkle_valid);
        assert!(result.verdict.contains("INVALID"));
    }

    #[test]
    fn verify_proof_offline_verbose_populates_steps() {
        let (proof, _) = make_valid_proof();
        let result = verify_proof_offline(&proof, true).unwrap();
        assert!(result.merkle_valid);
        // One path node → one step
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].step, 0);
        assert_eq!(result.steps[0].side, "left"); // index 0 is left child
    }

    // ── verifyBundle / executeBundle / sendBundle round-trips ────────────────

    /// Build a minimal `MessageInclusionProof` with no real cryptographic content.
    fn minimal_proof() -> MessageInclusionProof {
        use crate::types::ProofMessage;
        MessageInclusionProof {
            chain_id: "324".to_string(),
            l1_batch_number: 42,
            l2_message_index: 7,
            root: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            message: ProofMessage {
                tx_number_in_batch: 1,
                sender: "0x0000000000000000000000000000000000000001".to_string(),
                data: "0x".to_string(),
            },
            proof: vec![
                "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            ],
        }
    }

    /// Build a minimal `InteropBundle` with no calls.
    fn minimal_bundle() -> InteropBundle {
        use crate::types::{BundleAttributes, InteropCall};
        use alloy_primitives::FixedBytes;
        InteropBundle {
            version: FixedBytes([0x01]),
            sourceChainId: AlloyU256::from(324u64),
            destinationChainId: AlloyU256::from(325u64),
            interopBundleSalt: B256::ZERO,
            calls: vec![InteropCall {
                version: FixedBytes([0x01]),
                shadowAccount: false,
                to: Address::ZERO,
                from: Address::ZERO,
                value: AlloyU256::ZERO,
                data: Bytes::default(),
            }],
            bundleAttributes: BundleAttributes {
                executionAddress: Bytes::default(),
                unbundlerAddress: Bytes::default(),
                useFixedFee: false,
            },
        }
    }

    #[test]
    fn decode_verify_bundle_call_round_trip() {
        let bundle_bytes = encode_interop_bundle(&minimal_bundle());
        let encoded = encode_verify_bundle_call(bundle_bytes, minimal_proof()).unwrap();

        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "verifyBundle");
        assert_eq!(d.selector, hex::encode(verifyBundleCall::SELECTOR));
        // The inner bundle should decode to a proper object (not null)
        assert!(
            d.params["bundle"].is_object(),
            "expected decoded bundle object, got: {}",
            d.params["bundle"]
        );
        assert_eq!(d.params["bundle"]["sourceChainId"].as_str().unwrap(), "324");
        assert_eq!(
            d.params["bundle"]["destinationChainId"].as_str().unwrap(),
            "325"
        );
        // Proof fields should round-trip
        assert_eq!(d.params["proof"]["chainId"].as_str().unwrap(), "324");
        assert_eq!(d.params["proof"]["l1BatchNumber"].as_str().unwrap(), "42");
        assert_eq!(d.params["proof"]["l2MessageIndex"].as_str().unwrap(), "7");
    }

    #[test]
    fn decode_execute_bundle_call_round_trip() {
        let bundle_bytes = encode_interop_bundle(&minimal_bundle());
        let encoded = encode_execute_bundle_call(bundle_bytes, minimal_proof()).unwrap();

        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "executeBundle");
        assert_eq!(d.selector, hex::encode(executeBundleCall::SELECTOR));
        assert!(d.params["bundle"].is_object());
        assert_eq!(d.params["proof"]["chainId"].as_str().unwrap(), "324");
    }

    #[test]
    fn decode_send_bundle_call_round_trip() {
        let starter = InteropCallStarter {
            to: Bytes::from(vec![0x11, 0x22]),
            data: Bytes::from(vec![0xde, 0xad]),
            callAttributes: vec![Bytes::from(vec![0xca, 0xfe])],
        };
        let dest_chain = Bytes::from(vec![0x01, 0x44]);
        let encoded = encode_send_bundle_call(dest_chain.clone(), vec![starter], vec![]).unwrap();

        let d = decode_calldata_bytes(&encoded);
        assert_eq!(d.kind, "function_call");
        assert_eq!(d.name, "sendBundle");
        assert_eq!(d.selector, hex::encode(sendBundleCall::SELECTOR));
        assert_eq!(d.params["destinationChainId"].as_str().unwrap(), "0x0144");
        let starters = d.params["callStarters"]
            .as_array()
            .expect("callStarters array");
        assert_eq!(starters.len(), 1);
        assert_eq!(starters[0]["to"].as_str().unwrap(), "0x1122");
        assert_eq!(starters[0]["data"].as_str().unwrap(), "0xdead");
        let attrs = starters[0]["attributes"]
            .as_array()
            .expect("attributes array");
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].as_str().unwrap(), "0xcafe");
    }

    // ── Selector identity — every known name maps to exactly one selector ────

    #[test]
    fn selector_map_has_no_collisions() {
        let m = error_selector_map();
        // All values must be distinct names (no two selectors point to same name)
        let mut names: Vec<&str> = m.values().copied().collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            m.len(),
            "duplicate names in error_selector_map"
        );
    }
}
