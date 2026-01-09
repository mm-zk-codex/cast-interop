use crate::types::{address_to_hex, b256_to_hex, format_hex, u256_to_string};
use crate::types::{BundleAttributesView, InteropBundle, InteropBundleView as BundleView};
use crate::types::{InteropCallView, MessageInclusionProof};
use alloy_primitives::ruint::aliases::U256;
use alloy_primitives::{keccak256, Address, Bytes, B256, U256 as AlloyU256, U8};
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

    function verifyBundle(bytes _bundle, MessageInclusionProofSol _proof);
    function executeBundle(bytes _bundle, MessageInclusionProofSol _proof);
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
        "InteropBundleSent(bytes32,bytes32,(bytes1,uint256,uint256,bytes32,(bytes1,bool,address,address,uint256,bytes)[],(bytes,bytes)))",
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
