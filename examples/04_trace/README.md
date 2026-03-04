# Bundle Trace example

Trace the full interop lifecycle of a source transaction in a single command.

Instead of running `bundle extract`, `debug proof`, `debug root`, `bundle status`, and scanning for execution logs separately, `bundle trace` does it all at once as a non-blocking snapshot.

## Prerequisites

- Local zkSync OS multi-chain running on ports 3050 (source) and 3051 (destination)
- A previously sent interop message (see `01_greeting` example)

## Usage

### Trace a source transaction

```shell
export MESSAGE_TX=0x277d63aeaa0ad66a7b7c7b48ff1a5a0395b543b9a72da62064fc9ce2be6f66dc

cargo run bundle trace \
  --rpc-src http://localhost:3050 \
  --rpc-dest http://localhost:3051 \
  --tx $MESSAGE_TX
```

Example output:

```
trace for tx 0x277d...66dc

[+] source_receipt: ok  blockNumber=5  status=true
[+] bundle_decoded: ok  bundleHash=0xabc...  sourceChainId=6565  destinationChainId=6566  callCount=1
[+] proof_available: ok  batchNumber=3  proofLength=8
[+] root_settled: ok  root=0x6a96...0f3e  batchNumber=3
[+] bundle_status: ok  bundleHash=0xabc...  status=FullyExecuted  statusCode=2
[+] execution_tx: ok  txHash=0xdef...  blockNumber=12
```

### JSON output

```shell
cargo run bundle trace \
  --rpc-src http://localhost:3050 \
  --rpc-dest http://localhost:3051 \
  --tx $MESSAGE_TX \
  --json
```

### Tracing a pending message

If the message hasn't been relayed yet, some steps will show `pending` or `skipped`:

```
[+] source_receipt: ok  blockNumber=5  status=true
[+] bundle_decoded: ok  bundleHash=0xabc...  callCount=1
[~] proof_available: pending  message=proof not yet available
[-] root_settled: skipped  message=skipped (proof not available)
[+] bundle_status: ok  status=Unreceived  statusCode=0
[-] execution_tx: skipped  message=bundle not yet fully executed
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--rpc-src` | Source chain RPC URL | - |
| `--chain-src` | Source chain alias | - |
| `--rpc-dest` | Destination chain RPC URL | - |
| `--chain-dest` | Destination chain alias | - |
| `--tx` | Source transaction hash | required |
| `--msg-index` | Message index in the transaction | 0 |
| `--scan-blocks` | How far back to scan for BundleExecuted | 1000 |
| `--json` | Emit JSON output | false |
