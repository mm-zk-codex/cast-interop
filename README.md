# cast-interop

`cast-interop` is a cast-like CLI focused on zkSync interop workflows. It helps you extract bundles, fetch proofs, wait for roots, and execute/verify bundles across chains without wiring up the RPC or ABI plumbing every time.


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

**Proof never appears**

* Ensure the source RPC supports `zks_getL2ToL1LogProof`.
* Check that the transaction is finalized before polling.

**Root mismatch**

* Make sure `--source-chain` uses the source chainId (not alias).
* Verify you’re using the correct batch number from the proof.

**Execute reverted**

* Confirm the destination chainId matches the bundle’s destination.
* Validate permissions: `executionAddress`/`unbundlerAddress` must match the signer.

**RPC missing finalized or getLogProof**

* Use `cast-interop debug rpc --chain <alias>` to confirm capabilities.
* Switch to a zkSync-native RPC if the method is unsupported.

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
