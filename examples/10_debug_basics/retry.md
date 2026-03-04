# Debug retry (with real failures + recovery)

This example shows **real-world interop failures** and how to recover **without resending the original message**.

We reuse the **Whitelist sync** contracts (`examples/03_whitelist`), because they can fail in realistic ways (e.g. wrong trusted sender), and the failure is easy to diagnose.

You will learn how to:
- capture and store `bundle` + `proof` into files
- attempt execution and observe a real revert
- use the tool to explain what failed
- fix the destination contract
- retry **execution of the same bundle** successfully (no resend)

> This stays “non-advanced”: no unbundling, no partial execution. Just: fail → diagnose → fix → retry.

---

## Prerequisites

- Local zkSync OS setup with two L2s (source: 3050, destination: 3051)
- Contracts from `examples/03_whitelist`
- `cast-interop` (this repo)
- `forge` and `cast`

---

## Step 1: Deploy WhitelistMirror on destination (3051)

```shell
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3

forge create examples/03_whitelist/WhitelistMirror.sol:WhitelistMirror \
  -r http://localhost:3051 \
  --private-key $PRIVATE_KEY \
  --broadcast

# Deployed to: 0x....
export MIRROR_ADDR=0x....
```

## Step 2: Deploy WhitelistSource on source (3050)
We need destination recipient bytes (ERC-7930) for (destChainId, MIRROR_ADDR).

```shell
export INTEROP_CENTER=0x0000000000000000000000000000000000010010

cast chain-id -r http://localhost:3051
# 6566 (example)
export DEST_CHAIN_ID=6566

cargo run encode 7930 --chain-id $DEST_CHAIN_ID --address $MIRROR_ADDR
# 0x...
export DEST_RECIPIENT=0x...
```

Deploy the source contract:

```shell
forge create examples/03_whitelist/WhitelistSource.sol:WhitelistSource \
  -r http://localhost:3050 \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --constructor-args $INTEROP_CENTER $DEST_RECIPIENT

# Deployed to: 0x....
export SOURCE_ADDR=0x....
```

## Intentional failure: DO NOT set trusted sender on the mirror
In a correct setup you would call `WhitelistMirror.setTrustedSender(...)`
Here we intentionally skip it to cause a real execution revert.


### Step 3: Send a whitelist update (source tx)
Pick an account:

```shell
export ACCOUNT=0x000000000000000000000000000000000000dEaD
```

Send an update:

```shell
cast send -r http://localhost:3050 \
  --private-key $PRIVATE_KEY \
  $SOURCE_ADDR \
  "add(address)" \
  $ACCOUNT

# tx hash: 0x....
export WL_TX=0x....
```

Inspect the source tx:

```shell
cargo run debug tx --rpc http://localhost:3050  $WL_TX
```

### Step 4: Capture bundle + proof into files (for retries)

```shell
cargo run bundle extract \
  --rpc http://localhost:3050 \
  --tx $WL_TX \
  --out /tmp/wl.bundle.hex

cargo run debug proof \
  --rpc http://localhost:3050 \
  --tx $WL_TX \
  --out /tmp/wl.proof.json
# Message inclusion proof obtained. Batch number is XX
export BATCH_NUM=XX
```


### Step 5: Wait for interop root on destination

```shell
cargo run debug root \
  --source-chain 6565 \
  --rpc http://localhost:3051 \
  --batch $BATCH_NUM
```


### Step 6: Attempt execution (EXPECTED TO FAIL)
Now execute the bundle on destination using the stored files:

```shell
cargo run bundle execute \
  --rpc http://localhost:3051 \
  --bundle /tmp/wl.bundle.hex \
  --proof /tmp/wl.proof.json \
  --private-key $PRIVATE_KEY
# Error: server returned an error response: error code 3: execution reverted: UNTRUSTED_SENDER, data: 0x89fd2c76...
```

The error message includes a raw hex blob after `data:`. The first 4 bytes (`0x89fd2c76`) are the ABI error selector — they identify which Solidity error was thrown. The remaining bytes are the ABI-encoded parameters.

Without tooling you would have to: compute `keccak256("UnauthorizedMessageSender(address,address)")`, verify it starts with `89fd2c76`, then manually ABI-decode two `address` values from the rest. That's tedious and error-prone.

Instead, decode it offline in one command — no RPC needed:

```shell
cargo run debug decode 0x89fd2c76<rest of revert hex>
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

This immediately tells you:
- Which interop error was thrown (`UnauthorizedMessageSender`)
- What the contract expected as the trusted sender (`0x000...000` — not set yet)
- What it actually received (`0xYourSourceContractAddress` — the source contract that sent the message)

The fix is clear: call `setTrustedSender` on the mirror with the source contract's address. The `debug decode` command covers all 18 known interop error selectors — you never need to look up a selector manually again.


### Step 7: (optional) verify the bundle

While the execution is failing, you can still verify that bundle itself is correct.
```shell
cargo run bundle verify \
  --rpc http://localhost:3051 \
  --bundle /tmp/wl.bundle.hex \
  --proof /tmp/wl.proof.json \
  --private-key $PRIVATE_KEY
# sent tx: XXX
```



### Recovery: fix destination config and retry execution (same bundle+proof)

Now we correctly set the trusted sender on the mirror.

First compute ERC-7930 bytes for (sourceChainId, SOURCE_ADDR):

```shell
cast chain-id -r http://localhost:3050
# 6565 (example)
export SRC_CHAIN_ID=6565

cargo run encode 7930 --chain-id $SRC_CHAIN_ID --address $SOURCE_ADDR
# 0x...
export TRUSTED_SENDER=0x...
```

Set it:

```shell
cast send -r http://localhost:3051 \
  --private-key $PRIVATE_KEY \
  $MIRROR_ADDR \
  "setTrustedSender(bytes)" \
  $TRUSTED_SENDER
```

Sanity check:

```shell
cast call -r http://localhost:3051 $MIRROR_ADDR "trustedSenderHash()(bytes32)"
# now non-zero
```

### Retry execution (this time it should succeed)

```shell
cargo run bundle execute \
  --rpc http://localhost:3051 \
  --bundle /tmp/wl.bundle.hex \
  --proof /tmp/wl.proof.json \
  --private-key $PRIVATE_KEY

# sent tx: 0x....
export EXECUTE_TX=0x....
```

Verify the result:

```shell

cast call -r http://localhost:3051 $MIRROR_ADDR "isWhitelisted(address)(bool)" $ACCOUNT
# true
```

## Failure mode #2: wrong destination RPC / wrong chain
A very common mistake is executing against the wrong chain.

Try (intentionally wrong):

```shell
cargo run bundle execute \
  --rpc http://localhost:3050 \
  --bundle /tmp/wl.bundle.hex \
  --proof /tmp/wl.proof.json \
  --private-key $PRIVATE_KEY
# Execution reverted ..
```

Expected:

* it should fail because destinationChainId inside the bundle won’t match block.chainid.

How to diagnose — two options:

1. Inspect the bundle details with `bundle explain`:

```shell
cargo run bundle explain --rpc http://localhost:3050 --bundle /tmp/wl.bundle.hex --proof /tmp/wl.proof.json
# ...
# ❌ bundle.destinationChainId: bundle destination 6566 does not match current chain 6565
# ...
```

2. Decode the raw revert hex returned by the RPC using `debug decode` (offline, no RPC needed):

```shell
cargo run debug decode 0x4534e972<rest of revert hex>
# ❌ kind:     error
#    name:     WrongDestinationChainId
#    selector: 0x4534e972
#    params:
#      {
#        "bundleHash": "0x...",
#        "expected":   "6566",
#        "actual":     "6565"
#      }
```

How to recover:

just run the same execute command against the correct destination RPC.

### Cross-checking chain configuration

If you see `WrongDestinationChainId` failures when using `--chain` aliases — even though you’re pretty sure you’re targeting the right network — the problem may be in your stored config, not in the bundle.

When `chains add` is run, it probes the RPC and stores the live chainId. If the network is later reconfigured (testnet reset, environment change), the stored value becomes stale. The relay flow uses this stored chainId when building proof lookups — so every bundle will target the wrong chain ID, causing failures that look like bundle problems but are actually config problems.

Validate all aliases at once:

```shell
cargo run chains validate
```

This checks each alias for:
- **RPC reachability** — is the endpoint actually responding?
- **chainId match** — does the stored chainId match what the live RPC reports? A mismatch is the silent root cause of many `WrongDestinationChainId` failures when using `--chain` aliases
- **zkSync method availability** — does the RPC support `zks_getL2ToL1LogProof` and `zks_getL1BatchNumber`? A generic public RPC that doesn’t support these will fail proof fetching in ways that look like network errors rather than missing capability

Fix a stale chain entry:

```shell
cargo run chains rm test
cargo run chains add test --rpc http://localhost:3051
cargo run chains validate test   # confirm fixed
```

