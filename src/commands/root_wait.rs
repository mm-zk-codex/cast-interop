use crate::abi::{decode_bytes32, encode_interop_roots_call};
use crate::cli::RootWaitArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{parse_b256, parse_u256, AddressBook};
use alloy_primitives::{B256, U256};
use anyhow::Result;
use std::time::Duration;

pub async fn run(args: RootWaitArgs, _config: Config, addresses: AddressBook) -> Result<()> {
    let client = RpcClient::new(&args.rpc).await?;
    let chain_id = parse_u256(&args.source_chain)?;
    let expected_root = parse_b256(&args.expected_root)?;
    let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(300_000));
    let poll = Duration::from_millis(args.poll_ms.unwrap_or(1_000));
    let start = tokio::time::Instant::now();

    loop {
        let data = encode_interop_roots_call(chain_id, U256::from(args.batch));
        let result = eth_call(&client, addresses.interop_root_storage, data).await?;
        let root = decode_bytes32(result)?;
        if root != B256::ZERO {
            if root == expected_root {
                println!("interop root available: {root:#x}");
                return Ok(());
            }
            anyhow::bail!("interop root mismatch: expected {expected_root:#x}, got {root:#x}");
        }
        if start.elapsed() > timeout {
            anyhow::bail!("interop root did not become available in time");
        }
        tokio::time::sleep(poll).await;
    }
}
