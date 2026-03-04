use crate::abi::{
    decode_bytes32, encode_interop_roots_call, verify_proof_offline, ProofVerifyResult,
};
use crate::cli::ProofVerifyArgs;
use crate::config::Config;
use crate::rpc::{eth_call, RpcClient};
use crate::types::{AddressBook, MessageInclusionProof, BUNDLE_IDENTIFIER};
use alloy_primitives::{Bytes, B256, U256 as AlloyU256};
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::str::FromStr;

/// Extended result that adds an optional live on-chain root check to the offline result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FullVerifyOutput {
    #[serde(flatten)]
    offline: ProofVerifyResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_root: Option<LiveRootCheck>,
    /// Overall verdict combining offline Merkle check and (if present) live root check.
    overall_valid: bool,
    overall_verdict: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveRootCheck {
    dest_chain: String,
    stored_root: String,
    matches_computed: bool,
}

/// Verify a `MessageInclusionProof` offline and optionally against live chain state.
///
/// Reconstructs the Merkle leaf hash from the proof's message fields, walks the
/// Merkle tree, and compares the computed root to `proof.root`. If `--dest-chain`
/// is provided, also queries `interopRoots(chainId, batchNumber)` to confirm the
/// root is actually stored on the destination chain.
pub async fn run(args: ProofVerifyArgs, config: Config, addresses: AddressBook) -> Result<()> {
    // ── Load proof ───────────────────────────────────────────────────────────
    let mut proof = load_proof(&args.proof)?;

    // ── Patch message data if bundle is provided ─────────────────────────────
    // The proof returned by `debug proof` has data="0x" (unknown at fetch time).
    // The relay flow patches it to "0x01" + hex(abi_encode(bundle)).  If the user
    // supplies --bundle we do the same patching here so the leaf hash is correct.
    if let Some(bundle_hex) = &args.bundle {
        let bundle_bytes = load_hex_or_path(bundle_hex).context("failed to load --bundle")?;
        proof.message.data = format!(
            "0x{}{}",
            hex::encode([BUNDLE_IDENTIFIER]),
            hex::encode(&bundle_bytes)
        );
    } else if proof.message.data == "0x" || proof.message.data.is_empty() {
        eprintln!(
            "warning: proof.message.data is empty — leaf hash will be computed from empty data.\n\
             Pass --bundle <hex> to include the actual bundle bytes (required for a correct check)."
        );
    }

    // ── Offline Merkle verification ──────────────────────────────────────────
    let offline =
        verify_proof_offline(&proof, args.verbose).context("offline Merkle verification failed")?;

    // ── Live root check (optional) ───────────────────────────────────────────
    let live_root = if let Some(dc) = &args.dest_chain {
        Some(check_live_root(&proof, dc, &config, &addresses, &offline.computed_root).await?)
    } else {
        None
    };

    let overall_valid =
        offline.merkle_valid && live_root.as_ref().map_or(true, |r| r.matches_computed);
    let overall_verdict = build_overall_verdict(&offline, live_root.as_ref(), overall_valid);

    let output = FullVerifyOutput {
        offline,
        live_root,
        overall_valid,
        overall_verdict: overall_verdict.clone(),
    };

    // ── Output ───────────────────────────────────────────────────────────────
    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    print_human(&output, args.verbose);
    Ok(())
}

/// Query `interopRoots(chainId, batchNumber)` on the destination chain and
/// compare it to the computed Merkle root.
async fn check_live_root(
    proof: &MessageInclusionProof,
    dest_chain: &str,
    config: &Config,
    addresses: &AddressBook,
    computed_root_hex: &str,
) -> Result<LiveRootCheck> {
    // Resolve dest chain — try as alias first, then as raw RPC URL
    let resolved = config
        .resolve_rpc(None, Some(dest_chain))
        .or_else(|_| config.resolve_rpc(Some(dest_chain), None))
        .with_context(|| format!("could not resolve dest-chain '{dest_chain}'"))?;

    let client = RpcClient::new(&resolved.url)
        .await
        .with_context(|| format!("failed to connect to dest-chain '{dest_chain}'"))?;

    let chain_id = AlloyU256::from_str(&proof.chain_id)
        .map_err(|e| anyhow!("invalid chain_id '{}': {e}", proof.chain_id))?;
    let batch_number = AlloyU256::from(proof.l1_batch_number);

    let call = encode_interop_roots_call(chain_id, batch_number);
    let result = eth_call(&client, addresses.interop_root_storage, call)
        .await
        .context("interopRoots call failed on dest chain")?;

    let stored_root: B256 = decode_bytes32(Bytes::from(result.to_vec()))
        .context("failed to decode interopRoots return value")?;

    let computed_root =
        B256::from_str(computed_root_hex.trim_start_matches("0x")).unwrap_or(B256::ZERO);

    Ok(LiveRootCheck {
        dest_chain: dest_chain.to_string(),
        stored_root: format!("{stored_root:#x}"),
        matches_computed: stored_root == computed_root,
    })
}

fn build_overall_verdict(
    offline: &ProofVerifyResult,
    live: Option<&LiveRootCheck>,
    _overall_valid: bool,
) -> String {
    if !offline.merkle_valid {
        return format!("INVALID — Merkle verification failed: {}", offline.verdict);
    }
    match live {
        None => offline.verdict.clone(),
        Some(r) if !r.matches_computed => format!(
            "NOT YET READY — proof is cryptographically valid but interopRoots({}, {}) \
             is not yet on dest chain '{}' (stored: {}). \
             Wait for root propagation and retry.",
            offline.chain_id, offline.l1_batch_number, r.dest_chain, r.stored_root
        ),
        Some(r) => format!(
            "VALID — proof is cryptographically correct and interopRoots({}, {}) \
             is confirmed on dest chain '{}'; safe to call bundle verify/execute.",
            offline.chain_id, offline.l1_batch_number, r.dest_chain
        ),
    }
}

/// Print a human-readable verification report.
fn print_human(out: &FullVerifyOutput, verbose: bool) {
    let o = &out.offline;
    println!("source chain:  {}", o.chain_id);
    println!("batch:         {}", o.l1_batch_number);
    println!("leaf index:    {}", o.leaf_index);
    println!(
        "proof format:  {} ({} node(s))",
        if o.new_format {
            "new (metadata header)"
        } else {
            "legacy (plain path)"
        },
        o.proof_nodes_used
    );
    println!("leaf hash:     {}", o.leaf_hash);

    if verbose && !o.steps.is_empty() {
        println!("\nMerkle walk ({} steps):", o.steps.len());
        for s in &o.steps {
            println!(
                "  step {:>2}: idx={}, {} child → {}",
                s.step, s.index, s.side, s.hash
            );
        }
    }

    let merkle_icon = if o.merkle_valid { "✅" } else { "❌" };
    println!("\n{merkle_icon} computed root:  {}", o.computed_root);
    println!("{merkle_icon} proof.root:     {}", o.expected_root);
    if !o.merkle_valid {
        println!("   (roots differ — Merkle proof is invalid)");
    }

    if let Some(r) = &out.live_root {
        let root_icon = if r.matches_computed { "✅" } else { "⚠️ " };
        println!(
            "{root_icon} interopRoots({}, {}) on '{}': {}",
            o.chain_id, o.l1_batch_number, r.dest_chain, r.stored_root
        );
        if !r.matches_computed {
            println!("   (root not yet propagated to dest chain)");
        }
    }

    let overall_icon = if out.overall_valid { "✅" } else { "❌" };
    println!("\n{overall_icon} {}", out.overall_verdict);
}

/// Load a proof from a JSON file path or an inline JSON string.
fn load_proof(value: &str) -> Result<MessageInclusionProof> {
    let trimmed = value.trim();
    if std::path::Path::new(trimmed).exists() {
        let contents = std::fs::read_to_string(trimmed)
            .with_context(|| format!("failed to read proof file '{trimmed}'"))?;
        return serde_json::from_str(&contents).context("proof file is not valid JSON");
    }
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).context("inline proof JSON is invalid");
    }
    anyhow::bail!("proof must be a JSON file path or an inline JSON object")
}

/// Load raw bytes from a hex string or hex file path.
fn load_hex_or_path(value: &str) -> Result<Vec<u8>> {
    let trimmed = value.trim();
    let raw = if std::path::Path::new(trimmed).exists() {
        std::fs::read_to_string(trimmed)
            .with_context(|| format!("failed to read file '{trimmed}'"))?
    } else {
        trimmed.to_string()
    };
    let raw = raw.trim().trim_start_matches("0x");
    hex::decode(raw).map_err(|e| anyhow!("invalid hex: {e}"))
}
