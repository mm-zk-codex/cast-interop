use crate::types::{address_to_hex, b256_to_hex, format_hex, u256_to_string};
use crate::types::{BundleAttributesView, InteropBundle, InteropBundleView as BundleView};
use crate::types::{InteropCallView, MessageInclusionProof};
use alloy_primitives::ruint::aliases::U256;
use alloy_primitives::{keccak256, Address, Bytes, B256, U256 as AlloyU256, U8};
use alloy_sol_types::{SolCall, SolValue};
use anyhow::{anyhow, Result};
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
