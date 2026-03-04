mod common;

use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
#[ignore = "requires scripts/test-e2e.sh to provide live RPC endpoints"]
async fn bundle_relay_success() -> Result<()> {
    let env = common::TestEnv::from_env()?;
    common::wait_for_rpc_chain_id(&env.l1_rpc, 31_337, Duration::from_secs(10)).await?;
    common::wait_for_rpc_chain_id(&env.rpc_a, env.chain_a_id, Duration::from_secs(10)).await?;
    common::wait_for_rpc_chain_id(&env.rpc_b, env.chain_b_id, Duration::from_secs(10)).await?;
    let initial_message = "initialized";
    let final_message = "hello from A";
    let contract = common::deploy_greeting_contract(&env.rpc_b, &env.private_key).await?;
    let contract_address = format!("{contract:#x}");
    let message = common::read_message(&env.rpc_b, contract).await?;
    assert_eq!(message, initial_message);
    let payload = common::encode_message_payload(final_message);

    let send = common::run_cli(&[
        "send",
        "message",
        "--rpc",
        &env.rpc_a,
        "--to-chain",
        &env.chain_b_id.to_string(),
        "--to",
        &contract_address,
        "--payload",
        &payload,
        "--private-key",
        &env.private_key,
        "--json",
    ])?;
    let send_json = send.success_json()?;
    if !send_json["status"].as_bool().unwrap_or(false) {
        anyhow::bail!("send message returned a false status: {}", send.stdout);
    }
    let source_tx_hash = send_json["txHash"]
        .as_str()
        .context("send message response missing txHash")?
        .to_string();

    let out_dir = common::temp_test_dir("bundle-relay-success")?;
    let relay = common::run_cli(&[
        "bundle",
        "relay",
        "--rpc-src",
        &env.rpc_a,
        "--rpc-dest",
        &env.rpc_b,
        "--tx",
        &source_tx_hash,
        "--private-key",
        &env.private_key,
        "--json",
        "--out-dir",
        out_dir
            .to_str()
            .context("temp output directory was not valid UTF-8")?,
    ])?;
    let relay_json = relay.success_json()?;
    let bundle_hash = relay_json["bundleHash"]
        .as_str()
        .context("bundle relay response missing bundleHash")?;
    let handler_tx_hash = relay_json["handlerTxHash"]
        .as_str()
        .context("bundle relay response missing handlerTxHash")?;
    if handler_tx_hash.is_empty() {
        anyhow::bail!("handlerTxHash was empty");
    }
    let receipt_ok = wait_for_receipt_success(&env.rpc_b, handler_tx_hash).await?;
    if !receipt_ok {
        anyhow::bail!("handler transaction {handler_tx_hash} did not succeed");
    }

    common::assert_file_exists(&out_dir.join("bundle.hex"))?;
    common::assert_file_exists(&out_dir.join("proof.json"))?;
    common::assert_file_exists(&out_dir.join("relay_summary.json"))?;

    let bundle_status = wait_for_bundle_status(&env.rpc_b, bundle_hash).await?;
    if bundle_status != "FullyExecuted" && bundle_status != "Unbundled" {
        anyhow::bail!("unexpected bundle status {bundle_status}");
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires scripts/test-e2e.sh to provide live RPC endpoints"]
async fn bundle_relay_missing_receipt_fails() -> Result<()> {
    let env = common::TestEnv::from_env()?;
    common::wait_for_rpc_chain_id(&env.l1_rpc, 31_337, Duration::from_secs(10)).await?;
    common::wait_for_rpc_chain_id(&env.rpc_a, env.chain_a_id, Duration::from_secs(10)).await?;

    let relay = common::run_cli(&[
        "bundle",
        "relay",
        "--rpc-src",
        &env.rpc_a,
        "--rpc-dest",
        &env.rpc_b,
        "--tx",
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        "--private-key",
        &env.private_key,
    ])?;

    if relay.status_code == Some(0) {
        anyhow::bail!("bundle relay unexpectedly succeeded");
    }

    let combined = relay.combined_output();
    if !combined.contains("transaction receipt not found") {
        anyhow::bail!("expected missing receipt error, got output:\n{}", combined);
    }

    Ok(())
}

#[test]
fn parse_json_output_shape_examples() -> Result<()> {
    let send: Value =
        serde_json::from_str(r#"{"txHash":"0x1","status":true,"sendId":"0x2","bundleHash":null}"#)?;
    let relay: Value = serde_json::from_str(
        r#"{"sourceChainId":"6565","destinationChainId":"6566","l1BatchNumber":1,"l2MessageIndex":0,"bundleHash":"0x3","sourceTxHash":"0x4","handlerTxHash":"0x5"}"#,
    )?;
    let status: Value = serde_json::from_str(
        r#"{"bundleHash":"0x3","bundleStatus":"FullyExecuted","calls":null}"#,
    )?;

    assert_eq!(send["txHash"], "0x1");
    assert_eq!(relay["bundleHash"], "0x3");
    assert_eq!(status["bundleStatus"], "FullyExecuted");
    Ok(())
}

async fn wait_for_bundle_status(rpc_url: &str, bundle_hash: &str) -> Result<String> {
    for _ in 0..40 {
        let status = common::run_cli(&[
            "bundle",
            "status",
            "--rpc",
            rpc_url,
            "--bundle-hash",
            bundle_hash,
            "--json",
        ])?;
        let status_json = status.success_json()?;
        let bundle_status = status_json["bundleStatus"]
            .as_str()
            .context("bundle status response missing bundleStatus")?;
        if bundle_status != "Unreceived" {
            return Ok(bundle_status.to_string());
        }
        sleep(Duration::from_millis(500)).await;
    }
    anyhow::bail!("bundle status did not advance past Unreceived in time")
}

async fn wait_for_receipt_success(rpc_url: &str, tx_hash: &str) -> Result<bool> {
    for _ in 0..40 {
        if let Some(success) = common::fetch_receipt_status(rpc_url, tx_hash).await? {
            return Ok(success);
        }
        sleep(Duration::from_millis(500)).await;
    }
    anyhow::bail!("transaction receipt did not become available in time")
}
