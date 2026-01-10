use alloy_primitives::{address, Address, Bytes, B256, U256};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const DEFAULT_INTEROP_CENTER: &str = "0x0000000000000000000000000000000000010010";
pub const DEFAULT_INTEROP_HANDLER: &str = "0x000000000000000000000000000000000001000d";
pub const DEFAULT_INTEROP_ROOT_STORAGE: &str = "0x0000000000000000000000000000000000010008";
pub const BUNDLE_IDENTIFIER: u8 = 0x01;

pub const L1_SENDER_ADDRESS: Address = address!("0000000000000000000000000000000000008008");
pub const INTEROP_CENTER_ADDRESS: Address = address!("0000000000000000000000000000000000010010");

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
