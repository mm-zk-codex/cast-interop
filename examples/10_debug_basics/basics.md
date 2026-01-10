# Debug basics example

This example explains **how to debug interop step by step** using the CLI.

We reuse the `Greeting` contract from `examples/01_greeting`, but instead of using the high-level `relay` command, we **inspect, extract, store, and execute each step manually**.

This is useful when:
- something fails and you want to understand *where*
- you want to retry later
- you want to store intermediate artifacts (bundle, proof) in files
- you want full control over execution

---

## Prerequisites

- Local zkSync OS setup with two L2s
- The `Greeting` contract from `examples/01_greeting`
- `cast-interop` (this repo)
- `forge` and `cast`

---

## Step 1: Deploy the Greeting contract on the destination chain

We deploy the contract exactly like in `01_greeting`.

```shell
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3

forge create examples/01_greeting/Greeting.sol:Greeting \
  -r http://localhost:3051 \
  --private-key $PRIVATE_KEY \
  --broadcast

# Deployed to: 0x163CFa0911B9C7166b2608F0E902Fcd341523552
export CONTRACT_ADDR=0x163CFa0911B9C7166b2608F0E902Fcd341523552
```

Check initial state:

```shell
cast call -r http://localhost:3051 $CONTRACT_ADDR "message()(string)"
# "initialized"
```

## Step 2: Send a cross-chain message (source chain)
We send a greeting message from the source chain (3050).

```shell
cast abi-encode "f(string)" "hello from debug" > /tmp/message

cargo run send message \
  --rpc http://localhost:3050 \
  --to-chain 6566 \
  --to $CONTRACT_ADDR \
  --payload-file /tmp/message \
  --private-key $PRIVATE_KEY

# tx hash: 0x277d63aeaa0ad66a7b7c7b48ff1a5a0395b543b9a72da62064fc9ce2be6f66dc
# sendId: 0x9171a32f9cb0bfe359e4d4c1f6c6440ee5913ea9aa9c091a94fd31c92af1bdc7
```


Save the transaction hash — we will debug it manually.

```shell
export MESSAGE_TX=0x277d63aeaa0ad66a7b7c7b48ff1a5a0395b543b9a72da62064fc9ce2be6f66dc
```

## Step 3: Inspect the source transaction
Let’s inspect what the transaction actually produced.

```shell
cargo run debug tx --rpc http://localhost:3050 $MESSAGE_TX
```

This command shows:

* detected interop events
* bundle hash
* destination chain id
* sendId(s)

At this point nothing has executed yet on the destination chain, but the your transaction on the source chain is getting finalized
behind the scenes, and the interop root is being shared with the destination chain.

## Step 4: Extract and store the bundle
Now we extract the encoded interop bundle and store it in a file.

```shell
cargo run bundle extract \
  --rpc http://localhost:3050 \
  --tx $MESSAGE_TX \
  --out /tmp/bundle.hex
```

This file contains the ABI-encoded InteropBundle that will later be executed on the destination chain.

## Step 5: Fetch and store the inclusion proof
Before execution, we need the message inclusion proof. It will be available only once your transaction is finalized (on local machine should happen within couple seconds).

```shell
cargo run debug proof \
  --rpc http://localhost:3050 \
  --tx $MESSAGE_TX \
  --out /tmp/proof.json
# Message inclusion proof obtained. Batch number is 55
export BATCH_NUMBER=55
```

The proof is now stored on disk and can be reused later.

Remember the batch number - this is the source chain batch number, that your transaction was included in.

## Step 6: Wait for the interop root on the destination chain
The destination chain must first receive the interop root.

```shell
cargo run debug root \
  --source-chain 6565 \
  --rpc http://localhost:3051 \
  --batch $BATCH_NUMBER
```

This command waits until the root is available and prints it.

## Step 7: Execute the bundle manually
Now we execute the bundle on the destination chain using the files we saved.

```shell
cargo run bundle execute \
  --rpc http://localhost:3051 \
  --bundle /tmp/bundle.hex \
  --proof /tmp/proof.json \
  --private-key $PRIVATE_KEY
# sent tx: 0x8b0baaaa5069765d04cff6014cf1d91364c46d9abab066670a62d17bea7de5a3
export SENT_TX=0x8b0baaaa5069765d04cff6014cf1d91364c46d9abab066670a62d17bea7de5a3
```

This sends a transaction to the InteropHandler on the destination chain.


## Step 8: Verify the result
The greeting message should now be updated.

```shell

cast call -r http://localhost:3051 $CONTRACT_ADDR "message()(string)"
# "hello from debug"
```
You can also inspect the execution transaction:

```shell
cargo run debug tx --rpc http://localhost:3051 $SENT_TX
```

## What you learned
In this example you learned how to:

* inspect interop transactions (debug tx)
* extract and store bundles in files
* fetch and store proofs in files
* wait for interop roots
* manually execute bundles
* retry execution without resending messages

This workflow is extremely useful for:

* debugging failed relays
* operating interop manually
* building higher-level automation on top