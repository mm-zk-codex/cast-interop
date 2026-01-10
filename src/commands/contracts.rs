use crate::cli::ContractsArgs;
use crate::config::Config;
use crate::rpc::RpcClient;
use crate::types::{address_to_hex, AddressBook};
use alloy_primitives::Address;
use alloy_provider::Provider;
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractRow {
    name: String,
    address: String,
    code_len: u64,
    deployed: bool,
    abi_found: bool,
}

pub async fn run(args: ContractsArgs, config: Config, addresses: AddressBook) -> Result<()> {
    let resolved = config.resolve_rpc(args.rpc.rpc.as_deref(), args.rpc.chain.as_deref())?;
    let client = RpcClient::new(&resolved.url).await?;

    let abi_dir = config.abi_dir();
    let mut rows = Vec::new();
    rows.push(
        build_row(
            "interop_center",
            addresses.interop_center,
            &client,
            &abi_dir,
            "InteropCenter.json",
        )
        .await?,
    );
    rows.push(
        build_row(
            "interop_handler",
            addresses.interop_handler,
            &client,
            &abi_dir,
            "InteropHandler.json",
        )
        .await?,
    );
    rows.push(
        build_row(
            "interop_root_storage",
            addresses.interop_root_storage,
            &client,
            &abi_dir,
            "MessageVerification.json",
        )
        .await?,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    println!(
        "{:<22} {:<44} {:<10} {}",
        "name", "address", "codeLen", "abi"
    );
    for row in rows {
        let deployed = if row.deployed {
            "deployed"
        } else {
            "NOT DEPLOYED"
        };
        let abi = if row.abi_found { "yes" } else { "no" };
        println!(
            "{:<22} {:<44} {:<10} {}",
            row.name,
            row.address,
            format!("{} ({})", row.code_len, deployed),
            abi
        );
    }

    Ok(())
}

async fn build_row(
    name: &str,
    address: Address,
    client: &RpcClient,
    abi_dir: &PathBuf,
    abi_file: &str,
) -> Result<ContractRow> {
    let code = client.provider.get_code_at(address).await?;
    let code_len = code.len() as u64;
    let deployed = code_len > 0;
    let abi_found = abi_dir.join(abi_file).exists();
    Ok(ContractRow {
        name: name.to_string(),
        address: address_to_hex(address),
        code_len,
        deployed,
        abi_found,
    })
}
