# cast-interop

`cast-interop` is a cast-like CLI focused on ZKsync interop workflows. It helps you extract bundles, fetch proofs, wait for roots, and execute/verify bundles across chains without wiring up the RPC or ABI plumbing every time.


## Quick start

Sending token from chain A to chain B:

```shell
cast-interop token send --token $TOKEN_ADDRESS --to $ADDRESS --rpc-src $RPC_A   --rpc-dest $RPC_B --private-key $PRIVATE_KEY --amount-wei $AMOUNT
```
(see examples/02_token/README.md for more details)

Viewing interop bundles/messages created by a given transaction:

```shell

cast-interop debug tx --rpc $RPC $TX_HASH
```

Relaying all the bundles from transaction from chain A to chain B:

```shell
cast-interop bundle relay --rpc-src $RPC_A --rpc-dest $RPC_B --tx $TX_HASH --private-key $PRIVATE_KEY
```

Sending bundle with a single remote-call message:

```shell
cast-interop send message --to-chain $DESTINATION_CHAIN_ID  --to $CONTRACT_ADDR  --rpc $RPC_A  --payload-file /tmp/message  --private-key $PRIVATE_KEY
```
(see examples/01_greeting/README.md for more details)

Automatically relay all the bundles between a set of chains:

```shell
cast-interop auto-relay --rpc $RPC_A $RPC_B $RPC_C   --private-key $PRIVATE_KEY
```


## Installation

```bash
cargo install cast-interop
```

Or build locally:

```bash
cargo build --release
```

Binary path:

```bash
./target/release/cast-interop --help
```

## Configuration

Config file location:

```
~/.config/cast-interop/config.toml
```

Add chains (RPC + chainId stored):

```bash
cast-interop chains add era --rpc https://mainnet.era.zksync.io
cast-interop chains add test --rpc https://sepolia.era.zksync.dev
```

List configured chains:

```bash
cast-interop chains list
```

Example output:

```
alias        chainId    rpc
era          324        https://mainnet.era.zksync.io
test         300        https://sepolia.era.zksync.dev
```

Validate configured chains (RPC reachable, stored chainId matches live, ZKsync methods available):

```bash
cast-interop chains validate          # all configured chains
cast-interop chains validate era      # one specific alias
```

Example output:

```
chain: era
  ✅ rpc_reachable: RPC reachable
  ✅ chain_id_match: chainId 324 matches live RPC
  ✅ zks_log_proof: zks_getL2ToL1LogProof supported
  ✅ zks_batch_number: zks_getL1BatchNumber supported

chain: test
  ✅ rpc_reachable: RPC reachable
  ❌ chain_id_match: stored chainId 300 does not match live chainId 270
       hint: Run: cast-interop chains rm test && cast-interop chains add test --rpc <URL>
  ...

2 chain(s) validated — 1 failure(s), 0 warning(s)
```

**When to use this:** A chainId mismatch is one of the most common and hardest-to-spot misconfiguration issues. When `chains add` is run, it probes and stores the live chainId at that moment. If the chain is later reconfigured (e.g. testnet reset, different environment), the stored chainId becomes stale. The relay flow will then embed the wrong chainId in every proof lookup — causing `WrongDestinationChainId` reverts or silent proof-verification failures that look like bugs in the protocol rather than the config.

Run `chains validate` any time:
- Relay operations fail unexpectedly after a testnet reset or environment switch
- You've added new chains and want to confirm they're pointing where you think
- Before starting `auto-relay` — a misconfigured chain will silently fail every single bundle it tries to relay

You can still use the legacy `[rpc]` config for backwards compatibility:

```toml
[rpc]
default = "https://mainnet.era.zksync.io"
```

Preferred new format:

```toml
[chains.era]
rpc = "https://mainnet.era.zksync.io"
chainId = 324

[chains.test]
rpc = "https://sepolia.era.zksync.dev"
chainId = 300

[addresses]
interop_center = "0x0000000000000000000000000000000000010010"
interop_handler = "0x000000000000000000000000000000000001000d"
interop_root_storage = "0x0000000000000000000000000000000000010008"
```

RPC selection rules:

* Use `--rpc <URL>` **or** `--chain <alias>` (not both).
* If neither is provided, the CLI uses the default chain if configured.

Signer flags (required for sending transactions unless using `--dry-run`):

* `--private-key <hex>`
* `--private-key-env <ENV>` (default: `PRIVATE_KEY`)

## Core workflows

### Relay a bundle end-to-end (verify + execute)

```bash
cast-interop bundle relay \
  --chain-src era \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --private-key $PRIVATE_KEY
```

Sample output (trimmed):

```
sent tx: 0x6b6c...e219
```

Relay summary output (trimmed, with `--json`):

```bash
cast-interop bundle relay \
  --chain-src era \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --mode execute \
  --json
```

```json
{
  "sourceChainId": "324",
  "destinationChainId": "300",
  "l1BatchNumber": 12345,
  "l2MessageIndex": 7,
  "bundleHash": "0x4f3c...a2b1",
  "sourceTxHash": "0xabc...def",
  "handlerTxHash": "0x6b6c...e219"
}
```

### Only verify

```bash
cast-interop bundle relay \
  --chain-src era \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --mode verify \
  --private-key $PRIVATE_KEY
```

### Dry-run / simulate execute

```bash
cast-interop bundle relay \
  --chain-src era \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --mode execute \
  --dry-run
```

### Manual steps

1) Extract bundle:

```bash
cast-interop bundle extract --chain era --tx 0xSOURCE_TX_HASH --out bundle.hex
```

2) Get proof:

```bash
cast-interop debug proof --chain era --tx 0xSOURCE_TX_HASH --msg-index 0 --out proof.json
```

2b) **Verify the proof offline before spending gas:**

```bash
cast-interop debug proof-verify proof.json \
  --bundle bundle.hex \
  --dest-chain test
```

This reconstructs the Merkle leaf hash from the proof's message fields, walks the Merkle tree in pure Rust, and compares the computed root to what the ZKsync RPC reported. With `--dest-chain` it also queries `interopRoots(chainId, batchNumber)` live to confirm the root is actually stored on the destination chain.

**Why not just use `bundle explain` (which also does a simulation)?**
`bundle explain` does an on-chain `eth_call` simulation, which has three problems when debugging a proof:
1. It needs the full relay setup ready (signer, bundle file, proof file, correct chain) — if any of those are wrong it fails for the wrong reason.
2. It can only say "would revert" or "would succeed" — it can't tell you *why* the proof is wrong or which Merkle node diverges.
3. Critically, it merges two very different failure modes into one: **"the proof is cryptographically invalid"** (wrong tx hash, wrong msg_index, corrupted nodes) and **"the root just hasn't propagated to the destination yet"** (normal timing, just wait). Without `proof-verify` you cannot distinguish these — you'd retry indefinitely wondering if you'll ever succeed.

`proof-verify` gives you three distinct verdicts:
- `VALID` — math checks out AND root is on-chain → call `bundle execute`
- `NOT YET READY` — proof is cryptographically correct BUT root isn't on dest chain yet → just wait, no action needed
- `INVALID` — Merkle computation fails → the proof itself is wrong, fetching it again from the same tx will produce the same result; check your tx hash and msg_index

Example output (healthy):

```
source chain:  324
batch:         42
leaf index:    7
proof format:  legacy (plain path) (17 node(s))
leaf hash:     0x3a8f...c291

✅ computed root:  0xd4e7...b012
✅ proof.root:     0xd4e7...b012
✅ interopRoots(324, 42) on 'test': 0xd4e7...b012

✅ VALID — proof is cryptographically correct and interopRoots(324, 42) is confirmed on dest chain 'test'; safe to call bundle verify/execute.
```

Example output (root not yet propagated — wait, don't panic):

```
✅ computed root:  0xd4e7...b012
✅ proof.root:     0xd4e7...b012
⚠️  interopRoots(324, 42) on 'test': 0x0000...0000
   (root not yet propagated to dest chain)

❌ NOT YET READY — proof is cryptographically valid but interopRoots(324, 42) is not yet on dest chain 'test'. Wait for root propagation and retry.
```

Use `--verbose` to print every Merkle step — useful when a proof has wrong nodes and you need to see exactly which level of the tree starts computing a different hash. Use `--json` for structured output.

3) Wait for root on destination:

```bash
cast-interop debug root \
  --chain test \
  --source-chain 324 \
  --batch 12345 \
  --expected-root 0xROOT
```

4) Execute bundle:

```bash
cast-interop bundle execute \
  --chain test \
  --bundle bundle.hex \
  --proof proof.json \
  --private-key $PRIVATE_KEY
```

### Send a message

```bash
cast-interop send message \
  --chain era \
  --to-chain test \
  --to 0xTargetAddress \
  --payload 0xdeadbeef \
  --interop-value 0 \
  --execution-address permissionless \
  --dry-run
```

### Send a bundle

`calls.json`:

```json
{
  "calls": [
    {
      "to": "0xTargetAddress",
      "data": "0xabcdef",
      "attributes": {
        "interopValue": "0",
        "indirect": null
      }
    }
  ]
}
```

Send bundle:

```bash
cast-interop send bundle \
  --chain era \
  --to-chain test \
  --calls calls.json \
  --bundle-execution-address permissionless \
  --bundle-unbundler 0xYourAddress \
  --private-key $PRIVATE_KEY
```

### Token bridging (minimal)

Send an ERC20 via interop (Type B flow):

```bash
cast-interop token send \
  --chain-src era \
  --chain-dest test \
  --token 0xTokenOnSource \
  --amount 100 \
  --to 0xRecipientOnDest \
  --private-key $PRIVATE_KEY
```

Dry-run (simulate only):

```bash
cast-interop token send \
  --chain-src era \
  --chain-dest test \
  --token 0xTokenOnSource \
  --amount-wei 1000000000000000000 \
  --to 0xRecipientOnDest \
  --dry-run
```

Check wrap info and destination balance:

```bash
cast-interop token info \
  --chain-src era \
  --chain-dest test \
  --token 0xTokenOnSource

cast-interop token balance \
  --chain-src era \
  --chain-dest test \
  --token 0xTokenOnSource \
  --to 0xRecipientOnDest
```

Debug checklist for stuck transfers:

```bash
cast-interop debug tx --chain era 0xSOURCE_TX_HASH
cast-interop debug proof --chain era --tx 0xSOURCE_TX_HASH
cast-interop debug root --chain test --source-chain 324 --batch <batch> --expected-root <root>
cast-interop bundle status --chain test --bundle-hash <bundleHash>
cast-interop bundle explain --chain test --bundle <bundle.hex> --proof <proof.json>
cast-interop debug doctor --chain test
cast-interop debug decode 0xREVERT_DATA_OR_CALLDATA    # decode revert hex offline
cast-interop debug proof-verify proof.json --bundle bundle.hex --dest-chain test  # verify proof offline
cast-interop chains validate                           # confirm chains are correctly configured
```

### Watch progress

```bash
cast-interop debug watch \
  --chain-src era \
  --chain-dest test \
  --tx 0xSOURCE_TX_HASH \
  --until executed
```

## Key concepts

* **txHash**: The L2 transaction hash that emitted an `InteropBundleSent` or `MessageSent` event.
* **bundleHash**: The hash of the interop bundle emitted by `InteropCenter.sendBundle`.
* **sendId**: A per-message ID emitted by `InteropCenter.sendMessage` (bundleHash + index).
* **proof**: Inclusion proof data returned by `zks_getL2ToL1LogProof` (batch number, log index, proof nodes).
* **root wait**: Checks `interopRoots(chainId, batchNumber)` until the expected root is available on the destination chain.

## Troubleshooting

### "I got a hex revert string and don’t know what error it means"

When a transaction reverts with raw hex data (e.g. `execution reverted: data: 0x89fd2c76...`), the 4-byte prefix is an ABI error selector. Without tooling you’d have to compute `keccak256` of every possible error name, compare manually, and then hand-decode the ABI-packed parameters. That’s minutes of work per failure.

```bash
cast-interop debug decode 0x89fd2c76<rest of hex>
```

```
❌ kind:     error
   name:     UnauthorizedMessageSender
   selector: 0x89fd2c76
   params:
     {
       "expected": "0x0000000000000000000000000000000000000000",
       "actual":   "0xYourSourceContractAddress"
     }
```

This works **offline** — no RPC, no signing, no chain state needed. Pass any hex from a failed transaction, a revert string, or raw calldata and it will identify the interop function or error and decode all parameters. It covers all 18 known interop error selectors and 7 function call selectors.

### "My relay failed but I don’t know if it’s the proof, the timing, or something else"

If `bundle execute` reverted and you’re not sure whether the proof is wrong, the root hasn’t propagated yet, or there’s a contract-level permission issue, start with:

```bash
cast-interop debug proof-verify proof.json --bundle bundle.hex --dest-chain test
```

This tells you one of three things — **before you spend any gas**:

| Verdict | Meaning | Action |
|---------|---------|--------|
| `VALID` | Proof is correct AND root is on-chain | Call `bundle execute` — it should succeed |
| `NOT YET READY` | Proof is mathematically correct, but root hasn’t arrived at dest | Wait, then retry. Your proof is fine. |
| `INVALID` | Merkle computation fails — proof itself is wrong | Re-check your tx hash and msg_index. Fetching the same proof again won’t help. |

Without this command, "NOT YET READY" and "INVALID" both look like a reverted `eth_call` with `MessageNotIncluded`. You’d have no way to know whether to wait or to refetch.

### "Relay keeps failing even though the bundle looks correct"

When relay operations fail with errors that don’t match anything obviously wrong with the bundle — especially after a testnet reset, environment change, or config copy-paste — check the chain config:

```bash
cast-interop chains validate
```

A **stale chainId in the config is the most common silent misconfiguration** in interop workflows. When you ran `chains add`, it stored the live chainId at that moment. If the network was later reconfigured, the stored value is now wrong. The relay flow uses this stored value when building proof lookups, so every bundle will be addressed to the wrong chain — producing `WrongDestinationChainId` or proof failures that look like protocol bugs rather than config issues.

The check also confirms whether each RPC supports the ZKsync-specific methods (`zks_getL2ToL1LogProof`, `zks_getL1BatchNumber`) that the proof and relay flows depend on — a generic public RPC node that doesn’t support these will cause failures that are easy to mistake for network problems.

Run `chains validate` any time relays fail unexpectedly after a config change. It’s also the recommended first step before starting `auto-relay`, since a misconfigured chain there will silently fail every single bundle it attempts.

### "Proof never appears"

* Ensure the source RPC supports `zks_getL2ToL1LogProof` — use `cast-interop chains validate` or `cast-interop debug rpc --chain <alias>` to confirm.
* Check that the transaction is finalized before polling. Use `cast-interop debug watch` to track finalization progress.

### "Root mismatch / interopRoots returns zero"

* Make sure `--source-chain` uses the source chainId (not an alias string).
* Verify you’re using the correct batch number from the proof JSON.
* Use `cast-interop debug proof-verify proof.json --bundle bundle.hex --dest-chain test` — the `NOT YET READY` verdict means the root simply hasn’t propagated yet, not that the proof is wrong.

### "RPC missing zks_ methods"

* Use `cast-interop debug rpc --chain <alias>` to confirm which methods the RPC supports.
* Use `cast-interop chains validate` for a structured pass/fail report across all configured chains.
* Switch to a ZKsync-native RPC if the method is unsupported.

## Output formats

Most commands support `--json` for structured output.

Example (`bundle status`):

```bash
cast-interop bundle status --chain test --bundle-hash 0xBUNDLE --json
```

```json
{
  "bundleHash": "0xBUNDLE",
  "bundleStatus": "Verified",
  "calls": [
    { "index": 0, "status": "Executed" }
  ]
}
```

Example (`chains list`):

```bash
cast-interop chains list --json
```

```json
[
  {
    "alias": "era",
    "rpc": "https://mainnet.era.zksync.io",
    "chainId": "324"
  }
]
```

Example (`debug tx`, trimmed):

```bash
cast-interop debug tx --chain era 0xSOURCE_TX_HASH
```

```
bundleHash: 0x4f3c...a2b1
interopEvents: 3
```

Example (`debug decode` — turn a raw revert selector into a named error with parameters):

```bash
cast-interop debug decode 0x4534e972<...params...>
```

```
❌ kind:     error
   name:     WrongDestinationChainId
   selector: 0x4534e972
   params:
     {
       "actual": "300",
       "bundleHash": "0xabc...",
       "expected": "324"
     }
```

Works for function calldata too — useful for inspecting bundle files or calldata from any interop transaction:

```bash
cast-interop debug decode $(cat bundle.hex)
```

```
📦 kind:     bundle_struct
   name:     InteropBundle
   selector:
   params:
     {
       "sourceChainId": "324",
       "destinationChainId": "300",
       ...
     }
```

JSON output:

```bash
cast-interop debug decode 0x4534e972<...params...> --json
```

```json
{
  "kind": "error",
  "name": "WrongDestinationChainId",
  "selector": "4534e972",
  "params": {
    "actual": "300",
    "bundleHash": "0xabc...",
    "expected": "324"
  }
}
```

Example (`chains validate`):

```bash
cast-interop chains validate --json
```

```json
[
  { "chain": "era", "name": "rpc_reachable",   "status": "ok",   "details": "RPC reachable" },
  { "chain": "era", "name": "chain_id_match",  "status": "ok",   "details": "chainId 324 matches live RPC" },
  { "chain": "era", "name": "zks_log_proof",   "status": "ok",   "details": "zks_getL2ToL1LogProof supported" },
  { "chain": "era", "name": "zks_batch_number","status": "ok",   "details": "zks_getL1BatchNumber supported" }
]
```
