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
# Error: server returned an error response: error code 3: execution reverted: UNTRUSTED_SENDER, data:...
```


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

How to diagnose:

inspect the bundle details:

```shell
cargo run bundle explain --rpc http://localhost:3050 --bundle /tmp/wl.bundle.hex --proof /tmp/wl.proof.json
# ...
# ❌ bundle.destinationChainId: bundle destination 6566 does not match current chain 6565
# ...
```

How to recover:

just run the same execute command against the correct destination RPC.

