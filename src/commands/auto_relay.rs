use crate::abi::{decode_interop_bundle_sent, interop_bundle_sent_topic, l1_message_sent_topic};
use crate::cli::AutoRelayArgs;
use crate::config::Config;
use crate::relay_flow::{build_message_proof, execute_bundle, wait_for_proof, wait_for_root};
use crate::rpc::{get_transaction_receipt, RpcClient};
use crate::types::L1_SENDER_ADDRESS;
use crate::types::{AddressBook, MessageInclusionProof};
use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockNumberOrTag, BlockTransactions};
use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobStage {
    Detected,
    WaitingProof,
    WaitingRoot,
    NeedsKey,
    Executing,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Job {
    id: String,
    src_index: usize,
    dest_index: usize,
    src_chain_id: u64,
    dest_chain_id: u64,
    source_tx_hash: B256,
    block_number: u64,
    tx_index: u64,
    msg_index: u32,
    bundle_hash: Option<B256>,
    encoded_bundle: Bytes,
    log_proof: Option<crate::rpc::LogProof>,
    proof: Option<MessageInclusionProof>,
    root_ready: bool,
    handler_tx_hash: Option<B256>,
    stage: JobStage,
    created_at: Instant,
    last_update: Instant,
    attempts: u64,
    last_error: Option<String>,
    in_progress: bool,
}

#[derive(Debug, Clone)]
struct ChainStatus {
    label: String,
    chain_id: Option<u64>,
    head: Option<u64>,
    latency_ms: Option<u128>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct ChainRuntime {
    label: String,
    rpc_url: String,
    chain_id: u64,
    client: RpcClient,
}

#[derive(Debug)]
struct AppState {
    start: Instant,
    signer_loaded: bool,
    chains: Vec<ChainStatus>,
    jobs: HashMap<String, Job>,
    job_order: VecDeque<String>,
    max_jobs: usize,
}

#[derive(Debug, Clone)]
struct DetectedJob {
    src_index: usize,
    dest_chain_id: u64,
    source_tx_hash: B256,
    block_number: u64,
    tx_index: u64,
    msg_index: u32,
    bundle_hash: Option<B256>,
    encoded_bundle: Bytes,
}

pub async fn run(args: AutoRelayArgs, _config: Config, addresses: AddressBook) -> Result<()> {
    if args.rpc.len() < 2 {
        anyhow::bail!("auto-relay requires at least two --rpc entries");
    }

    let signer = match args.private_key.as_deref() {
        Some(key) => Some(key.parse().context("invalid private key")?),
        None => None,
    };

    let mut chains = Vec::new();
    for (idx, url) in args.rpc.iter().enumerate() {
        let client = RpcClient::new(url).await?;
        let chain_id = client.provider.get_chain_id().await?;
        let label = chain_label(idx);
        chains.push(ChainRuntime {
            label,
            rpc_url: url.clone(),
            chain_id,
            client,
        });
    }

    let mut chain_status = Vec::new();
    for chain in &chains {
        chain_status.push(ChainStatus {
            label: chain.label.clone(),
            chain_id: Some(chain.chain_id),
            head: None,
            latency_ms: None,
            last_error: None,
        });
    }

    let state = Arc::new(Mutex::new(AppState {
        start: Instant::now(),
        signer_loaded: signer.is_some(),
        chains: chain_status,
        jobs: HashMap::new(),
        job_order: VecDeque::new(),
        max_jobs: args.max_jobs as usize,
    }));

    let semaphore = Arc::new(Semaphore::new(args.max_inflight as usize));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    for (index, chain) in chains.clone().into_iter().enumerate() {
        let state = Arc::clone(&state);
        let semaphore = Arc::clone(&semaphore);
        let shutdown_rx = shutdown_rx.clone();
        let lookback = args.lookback_blocks;
        let poll = Duration::from_millis(args.poll_interval_ms);
        let center = addresses.interop_center;
        tokio::spawn(async move {
            if let Err(err) = chain_poll_loop(
                index,
                chain,
                state,
                semaphore,
                lookback,
                poll,
                center,
                shutdown_rx,
            )
            .await
            {
                eprintln!("chain poll failed: {err}");
            }
        });
    }

    let job_state = Arc::clone(&state);
    let job_semaphore = Arc::clone(&semaphore);
    let job_shutdown_rx = shutdown_rx.clone();
    let job_chains = chains.clone();
    let handler = addresses.interop_handler;
    let root_storage = addresses.interop_root_storage;
    let center = addresses.interop_center;
    tokio::spawn(async move {
        job_processor_loop(
            job_state,
            job_chains,
            handler,
            root_storage,
            center,
            signer,
            job_semaphore,
            args.poll_interval_ms,
            job_shutdown_rx,
        )
        .await;
    });

    run_ui(state, shutdown_tx).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn chain_poll_loop(
    index: usize,
    chain: ChainRuntime,
    state: Arc<Mutex<AppState>>,
    semaphore: Arc<Semaphore>,
    lookback_blocks: u64,
    poll: Duration,
    center: Address,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let mut last_scanned: Option<u64> = None;
    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let poll_start = Instant::now();
        let head = match chain.client.provider.get_block_number().await {
            Ok(head) => head,
            Err(err) => {
                update_chain_error(&state, index, err.to_string());
                tokio::time::sleep(poll).await;
                continue;
            }
        };
        let head = head as u64;
        let start_block = match last_scanned {
            Some(last) if head > last => last + 1,
            Some(_) => {
                update_chain_status(&state, index, head, poll_start.elapsed());
                tokio::time::sleep(poll).await;
                continue;
            }
            None => head.saturating_sub(lookback_blocks),
        };

        for block_number in start_block..=head {
            if *shutdown_rx.borrow() {
                break;
            }
            if let Err(err) = scan_block(
                &chain.client,
                block_number,
                center,
                index,
                &state,
                &semaphore,
            )
            .await
            {
                update_chain_error(&state, index, err.to_string());
            }
        }
        last_scanned = Some(head);
        update_chain_status(&state, index, head, poll_start.elapsed());
        tokio::time::sleep(poll).await;
    }
    Ok(())
}

async fn scan_block(
    client: &RpcClient,
    block_number: u64,
    center: Address,
    src_index: usize,
    state: &Arc<Mutex<AppState>>,
    semaphore: &Arc<Semaphore>,
) -> Result<()> {
    let block = client
        .provider
        .get_block_by_number(BlockNumberOrTag::Number(block_number))
        .await?
        .ok_or_else(|| anyhow!("missing block {block_number}"))?;

    let tx_hashes: Vec<B256> = match block.transactions {
        BlockTransactions::Hashes(hashes) => hashes,
        BlockTransactions::Full(txs) => txs.into_iter().map(|tx| *tx.into_inner().hash()).collect(),
        _ => Vec::new(),
    };

    if tx_hashes.is_empty() {
        return Ok(());
    }

    let mut join_set = JoinSet::new();
    for hash in tx_hashes {
        let client = client.clone();
        let semaphore = Arc::clone(semaphore);
        join_set.spawn(async move {
            let _permit = semaphore.acquire_owned().await?;
            let receipt = get_transaction_receipt(&client, hash).await?;
            Ok::<_, anyhow::Error>(receipt)
        });
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(receipt)) => {
                for job in detect_jobs_from_receipt(&receipt, center, src_index) {
                    insert_job(state, job)?;
                }
            }
            Ok(Err(_)) => {
                // This will be too spammy.
            }
            Err(err) => {
                eprintln!("receipt task failed: {err}");
            }
        }
    }

    Ok(())
}

fn detect_jobs_from_receipt(
    receipt: &alloy_rpc_types::TransactionReceipt,
    center: Address,
    src_index: usize,
) -> Vec<DetectedJob> {
    let mut jobs = Vec::new();
    let mut l1_message_count: u32 = 0;
    let l1_messenger = L1_SENDER_ADDRESS;
    let l1_msg_topic = l1_message_sent_topic();
    let bundle_topic = interop_bundle_sent_topic();

    for log in receipt.logs() {
        let topic = log.topics().first().copied();

        // Count L1MessageSent events from L1Messenger (L2ToL1Messenger at 0x8008)
        if log.address() == l1_messenger && topic == Some(l1_msg_topic) {
            l1_message_count += 1;
        }

        // When we see an InteropBundleSent, create a job with the current L1 message count
        if log.address() == center && topic == Some(bundle_topic) {
            if let Ok((_, bundle_hash, bundle)) =
                decode_interop_bundle_sent(log.data().data.clone())
            {
                let encoded_bundle = crate::abi::encode_interop_bundle(&bundle);
                if let Ok(dest_chain_id) = u256_to_u64(bundle.destinationChainId) {
                    // msg_index is the count of L1MessageSent events before this bundle event,
                    // minus 1 because indices are 0-based and the bundle's own L1MessageSent
                    // is included in the count
                    let msg_index = l1_message_count.saturating_sub(1);
                    jobs.push(DetectedJob {
                        src_index,
                        dest_chain_id,
                        source_tx_hash: receipt.transaction_hash,
                        block_number: receipt.block_number.unwrap_or_default(),
                        tx_index: receipt.transaction_index.unwrap_or_default(),
                        msg_index,
                        bundle_hash: Some(bundle_hash),
                        encoded_bundle,
                    });
                }
            }
        }
    }
    jobs
}

fn insert_job(state: &Arc<Mutex<AppState>>, detected: DetectedJob) -> Result<()> {
    let mut state = state.lock().expect("state lock");
    // Include msg_index in key to differentiate multiple bundles from the same transaction
    let key = format!(
        "{}:{:#x}:{}",
        detected.src_index, detected.source_tx_hash, detected.msg_index
    );
    if state.jobs.contains_key(&key) {
        return Ok(());
    }

    let dest_index = state
        .chains
        .iter()
        .position(|chain| chain.chain_id == Some(detected.dest_chain_id));
    let Some(dest_index) = dest_index else {
        return Ok(());
    };

    let now = Instant::now();
    let job = Job {
        id: key.clone(),
        src_index: detected.src_index,
        dest_index,
        src_chain_id: state.chains[detected.src_index]
            .chain_id
            .unwrap_or_default(),
        dest_chain_id: detected.dest_chain_id,
        source_tx_hash: detected.source_tx_hash,
        block_number: detected.block_number,
        tx_index: detected.tx_index,
        msg_index: detected.msg_index,
        bundle_hash: detected.bundle_hash,
        encoded_bundle: detected.encoded_bundle,
        log_proof: None,
        proof: None,
        root_ready: false,
        handler_tx_hash: None,
        stage: JobStage::Detected,
        created_at: now,
        last_update: now,
        attempts: 0,
        last_error: None,
        in_progress: false,
    };
    eprintln!(
        "detected bundle {:#x} msg_index={} -> dest {}",
        job.source_tx_hash, job.msg_index, job.dest_chain_id
    );
    state.jobs.insert(key.clone(), job);
    state.job_order.push_back(key.clone());
    while state.job_order.len() > state.max_jobs {
        if let Some(old_key) = state.job_order.pop_front() {
            state.jobs.remove(&old_key);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn job_processor_loop(
    state: Arc<Mutex<AppState>>,
    chains: Vec<ChainRuntime>,
    handler: Address,
    root_storage: Address,
    center: Address,
    signer: Option<alloy_signer_local::PrivateKeySigner>,
    semaphore: Arc<Semaphore>,
    poll_interval_ms: u64,
    shutdown_rx: watch::Receiver<bool>,
) {
    let poll = Duration::from_millis(poll_interval_ms);
    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let mut to_process = Vec::new();
        {
            let mut state = state.lock().expect("state lock");
            for (key, job) in state.jobs.iter_mut() {
                if job.in_progress {
                    continue;
                }
                if matches!(job.stage, JobStage::Done | JobStage::NeedsKey) {
                    continue;
                }
                if matches!(job.stage, JobStage::Failed) {
                    continue;
                }
                job.in_progress = true;
                job.attempts += 1;
                to_process.push(key.clone());
            }
        }

        for key in to_process {
            let state = Arc::clone(&state);
            let chains = chains.clone();
            let semaphore = Arc::clone(&semaphore);
            let signer = signer.clone();
            tokio::spawn(async move {
                if let Err(err) = process_job(
                    &state,
                    &chains,
                    handler,
                    root_storage,
                    center,
                    signer,
                    semaphore,
                    &key,
                    poll,
                )
                .await
                {
                    update_job_failure(&state, &key, err.to_string());
                }
                clear_job_in_progress(&state, &key);
            });
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_job(
    state: &Arc<Mutex<AppState>>,
    chains: &[ChainRuntime],
    handler: Address,
    root_storage: Address,
    center: Address,
    signer: Option<alloy_signer_local::PrivateKeySigner>,
    semaphore: Arc<Semaphore>,
    job_key: &str,
    poll: Duration,
) -> Result<()> {
    let (stage, src_index, dest_index, tx_hash, block_number, msg_index, bundle, proof, root_ready) = {
        let state = state.lock().expect("state lock");
        let job = state.jobs.get(job_key).context("job missing")?;
        (
            job.stage,
            job.src_index,
            job.dest_index,
            job.source_tx_hash,
            job.block_number,
            job.msg_index,
            job.encoded_bundle.clone(),
            job.log_proof.clone(),
            job.root_ready,
        )
    };

    let source = &chains[src_index];
    let dest = &chains[dest_index];
    let timeout = Duration::from_millis(300_000);
    match stage {
        JobStage::Detected | JobStage::WaitingProof => {
            update_job_stage(state, job_key, JobStage::WaitingProof);
            let _permit = semaphore.acquire_owned().await?;
            let proof = wait_for_proof(
                &source.client,
                block_number,
                tx_hash,
                msg_index,
                timeout,
                poll,
            )
            .await
            .context("proof wait failed")?;
            eprintln!(
                "proof ready {:#x} batch={} id={}",
                tx_hash, proof.batch_number, proof.id
            );
            store_log_proof(state, job_key, proof, center);
            update_job_stage(state, job_key, JobStage::WaitingRoot);
        }
        JobStage::WaitingRoot => {
            if root_ready {
                if signer.is_some() {
                    update_job_stage(state, job_key, JobStage::Executing);
                } else {
                    update_job_stage(state, job_key, JobStage::NeedsKey);
                }
                return Ok(());
            }
            let proof = proof.context("missing log proof")?;
            let _permit = semaphore.acquire_owned().await?;
            wait_for_root(
                &dest.client,
                root_storage,
                source.chain_id,
                proof.batch_number,
                proof.root.clone(),
                timeout,
                poll,
            )
            .await
            .context("root wait failed")?;
            eprintln!("root ready {:#x} batch={}", tx_hash, proof.batch_number);
            mark_root_ready(state, job_key);
        }
        JobStage::Executing => {
            let proof = load_proof_from_state(state, job_key)?;
            let signer = signer.clone().ok_or_else(|| anyhow!("missing signer"))?;
            let tx_hash =
                execute_bundle(&dest.client, &dest.rpc_url, handler, signer, bundle, proof).await?;
            eprintln!("executed tx {tx_hash:#x}");
            mark_job_done(state, job_key, Some(tx_hash));
        }
        JobStage::NeedsKey | JobStage::Done | JobStage::Failed => {}
    }

    Ok(())
}

fn update_chain_status(state: &Arc<Mutex<AppState>>, index: usize, head: u64, latency: Duration) {
    let mut state = state.lock().expect("state lock");
    if let Some(chain) = state.chains.get_mut(index) {
        chain.head = Some(head);
        chain.latency_ms = Some(latency.as_millis());
        chain.last_error = None;
    }
}

fn update_chain_error(state: &Arc<Mutex<AppState>>, index: usize, err: String) {
    let mut state = state.lock().expect("state lock");
    if let Some(chain) = state.chains.get_mut(index) {
        chain.last_error = Some(short_error(&err));
    }
}

fn update_job_stage(state: &Arc<Mutex<AppState>>, job_key: &str, stage: JobStage) {
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        job.stage = stage;
        job.last_update = Instant::now();
    }
}

fn store_log_proof(
    state: &Arc<Mutex<AppState>>,
    job_key: &str,
    proof: crate::rpc::LogProof,
    center: Address,
) {
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        let message_proof = build_message_proof(
            &proof,
            job.tx_index,
            center,
            &job.encoded_bundle,
            job.src_chain_id,
        );
        job.log_proof = Some(proof);
        job.proof = Some(message_proof);
        job.last_update = Instant::now();
    }
}

fn mark_root_ready(state: &Arc<Mutex<AppState>>, job_key: &str) {
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        job.root_ready = true;
        job.last_update = Instant::now();
    }
}

fn mark_job_done(state: &Arc<Mutex<AppState>>, job_key: &str, tx_hash: Option<B256>) {
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        job.stage = JobStage::Done;
        job.handler_tx_hash = tx_hash;
        job.last_update = Instant::now();
    }
}

fn update_job_failure(state: &Arc<Mutex<AppState>>, job_key: &str, message: String) {
    if is_idempotent_error(&message) {
        mark_job_done(state, job_key, None);
        return;
    }
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        job.stage = JobStage::Failed;
        job.last_error = Some(short_error(&message));
        job.last_update = Instant::now();
        eprintln!("failed {job_key}: {}", message);
    }
}

fn clear_job_in_progress(state: &Arc<Mutex<AppState>>, job_key: &str) {
    let mut state = state.lock().expect("state lock");
    if let Some(job) = state.jobs.get_mut(job_key) {
        job.in_progress = false;
    }
}

fn load_proof_from_state(
    state: &Arc<Mutex<AppState>>,
    job_key: &str,
) -> Result<MessageInclusionProof> {
    let state = state.lock().expect("state lock");
    let job = state.jobs.get(job_key).context("job missing")?;
    job.proof.clone().context("proof missing")
}

async fn run_ui(state: Arc<Mutex<AppState>>, shutdown_tx: watch::Sender<bool>) -> Result<()> {
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let stdin = BufReader::new(tokio::io::stdin());
        let mut lines = stdin.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if input_tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut should_quit = false;
    let mut tick = tokio::time::interval(Duration::from_millis(200));

    while !should_quit {
        tokio::select! {
            _ = tick.tick() => {
                let snapshot = state.lock().expect("state lock").clone();
                let output = render_ui(&snapshot);
                print!("\x1B[2J\x1B[H{output}");
                io::stdout().flush().ok();
            }
            Some(line) = input_rx.recv() => {
                let command = line.trim();
                if command.eq_ignore_ascii_case("q") {
                    should_quit = true;
                } else if command.eq_ignore_ascii_case("r") {
                    retry_failed_jobs(&state);
                } else if command.eq_ignore_ascii_case("c") {
                    clear_done_jobs(&state);
                }
            }
        }
    }

    shutdown_tx.send(true).ok();
    Ok(())
}
fn render_ui(state: &AppState) -> String {
    let mut output = String::new();
    output.push_str(&format!("{}\n\n", build_header(state)));
    output.push_str(&format!("{}\n", build_chain_table(state)));
    output.push_str(&format!("{}\n", build_job_table(state)));
    output
}

fn build_header(state: &AppState) -> String {
    let signer = if state.signer_loaded { "yes" } else { "no" };
    let uptime = format_duration(state.start.elapsed());
    let chain_labels = state
        .chains
        .iter()
        .filter_map(|chain| chain.chain_id.map(|id| format!("{}:{}", chain.label, id)))
        .collect::<Vec<_>>()
        .join(" ");
    format!("AUTO-RELAY (EXECUTE) | signer: {signer} | chains: {chain_labels} | {uptime}")
}

fn build_chain_table(state: &AppState) -> String {
    let headers = ["Name", "ChainId", "Head", "Latency", "Last error"];
    let widths = [5, 12, 12, 10, 40];
    let mut rows = Vec::new();
    for chain in &state.chains {
        rows.push(vec![
            chain.label.clone(),
            chain.chain_id.map(|id| id.to_string()).unwrap_or_default(),
            chain.head.map(|h| h.to_string()).unwrap_or_default(),
            chain
                .latency_ms
                .map(|ms| format!("{ms}ms"))
                .unwrap_or_default(),
            chain.last_error.clone().unwrap_or_default(),
        ]);
    }
    format_table("Chains", &headers, &rows, &widths)
}

fn build_job_table(state: &AppState) -> String {
    let headers = ["Age", "Src→Dest", "Tx", "Stage", "Details"];
    let widths = [6, 10, 14, 12, 100];
    let mut rows = Vec::new();
    for key in state.job_order.iter().rev() {
        if let Some(job) = state.jobs.get(key) {
            let age = format_duration(job.created_at.elapsed());
            let src_label = &state.chains[job.src_index].label;
            let dest_label = &state.chains[job.dest_index].label;
            let tx = short_hash(job.source_tx_hash);
            let stage = stage_label(job.stage);
            let details = job_details(job);
            rows.push(vec![
                age,
                format!("{src_label}→{dest_label}"),
                tx,
                stage.to_string(),
                details,
            ]);
        }
    }
    format_table("Jobs", &headers, &rows, &widths)
}

fn format_table(title: &str, headers: &[&str], rows: &[Vec<String>], widths: &[usize]) -> String {
    let mut output = String::new();
    output.push_str(&format!("{title}\n"));
    output.push_str(&format_row(headers, widths));
    let separator: Vec<&str> = headers.iter().map(|_| "-").collect();
    output.push_str(&format_row(&separator, widths));
    for row in rows {
        let row_refs: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
        output.push_str(&format_row(&row_refs, widths));
    }
    output
}

fn format_row(values: &[&str], widths: &[usize]) -> String {
    let mut row = String::new();
    for (value, width) in values.iter().zip(widths.iter()) {
        row.push_str(&pad_cell(value, *width));
        row.push(' ');
    }
    row.push('\n');
    row
}

fn pad_cell(value: &str, width: usize) -> String {
    let mut out = String::new();
    let truncated = if value.len() > width {
        &value[..width.saturating_sub(1)]
    } else {
        value
    };
    out.push_str(truncated);
    let padding = width.saturating_sub(truncated.len());
    out.push_str(&" ".repeat(padding));
    out
}

fn short_hash(hash: B256) -> String {
    let full = format!("{hash:#x}");
    if full.len() <= 12 {
        return full;
    }
    format!("{}…{}", &full[..8], &full[full.len() - 4..])
}

fn stage_label(stage: JobStage) -> &'static str {
    match stage {
        JobStage::Detected => "DETECTED",
        JobStage::WaitingProof => "PROOF",
        JobStage::WaitingRoot => "WAIT_ROOT",
        JobStage::NeedsKey => "NEEDS_KEY",
        JobStage::Executing => "EXEC",
        JobStage::Done => "DONE",
        JobStage::Failed => "FAIL",
    }
}

fn job_details(job: &Job) -> String {
    match job.stage {
        JobStage::Detected => "waiting".to_string(),
        JobStage::WaitingProof => {
            if let Some(proof) = &job.log_proof {
                format!("batch {} id {}", proof.batch_number, proof.id)
            } else {
                "waiting proof".to_string()
            }
        }
        JobStage::WaitingRoot => {
            if let Some(proof) = &job.log_proof {
                short_value(&proof.root)
            } else {
                "waiting root".to_string()
            }
        }
        JobStage::NeedsKey => "awaiting key".to_string(),
        JobStage::Executing => "sending".to_string(),
        JobStage::Done => job
            .handler_tx_hash
            .map(short_hash)
            .unwrap_or_else(|| "done".to_string()),
        JobStage::Failed => job
            .last_error
            .clone()
            .unwrap_or_else(|| "failed".to_string()),
    }
}

fn short_value(value: &str) -> String {
    if value.len() <= 10 {
        value.to_string()
    } else {
        format!("{}…{}", &value[..6], &value[value.len() - 4..])
    }
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

fn retry_failed_jobs(state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().expect("state lock");
    let signer_loaded = state.signer_loaded;
    for job in state.jobs.values_mut() {
        if job.stage != JobStage::Failed {
            continue;
        }
        job.last_error = None;
        job.in_progress = false;
        if job.proof.is_none() {
            job.stage = JobStage::WaitingProof;
        } else if !job.root_ready {
            job.stage = JobStage::WaitingRoot;
        } else if signer_loaded {
            job.stage = JobStage::Executing;
        } else {
            job.stage = JobStage::NeedsKey;
        }
        job.last_update = Instant::now();
    }
}

fn clear_done_jobs(state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().expect("state lock");
    let mut to_remove = Vec::new();
    for key in state.job_order.iter() {
        if let Some(job) = state.jobs.get(key) {
            if job.stage == JobStage::Done {
                to_remove.push(key.clone());
            }
        }
    }
    for key in to_remove {
        state.jobs.remove(&key);
        state.job_order.retain(|k| k != &key);
    }
}

fn short_error(err: &str) -> String {
    let trimmed = err.lines().next().unwrap_or(err).trim();
    if trimmed.len() > 80 {
        format!("{}…", &trimmed[..77])
    } else {
        trimmed.to_string()
    }
}

fn is_idempotent_error(err: &str) -> bool {
    let lowered = err.to_lowercase();
    lowered.contains("already")
        || lowered.contains("bundlealreadyprocessed")
        || lowered.contains("bundleverifiedalready")
        || lowered.contains("callalreadyexecuted")
}

fn u256_to_u64(value: U256) -> Result<u64> {
    let bytes = value.to_be_bytes::<32>();
    if bytes[..24].iter().any(|byte| *byte != 0) {
        anyhow::bail!("chain id too large: {value}");
    }
    let mut tail = [0u8; 8];
    tail.copy_from_slice(&bytes[24..]);
    Ok(u64::from_be_bytes(tail))
}

fn chain_label(index: usize) -> String {
    let base = (b'A' + (index % 26) as u8) as char;
    if index < 26 {
        base.to_string()
    } else {
        format!("{base}{}", index / 26)
    }
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            start: self.start,
            signer_loaded: self.signer_loaded,
            chains: self.chains.clone(),
            jobs: self.jobs.clone(),
            job_order: self.job_order.clone(),
            max_jobs: self.max_jobs,
        }
    }
}
