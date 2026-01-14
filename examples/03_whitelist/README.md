# Whitelist sync example

This example shows how to keep a whitelist on a destination chain in sync with a source chain.

- On the **source chain**, `WhitelistSource` is the source of truth and sends interop messages when the whitelist changes.
- On the **destination chain**, `WhitelistMirror` receives those messages and updates `isWhitelisted(address)`.

This is a great “basic” interop use-case: no tokens, no bridges — just cross-chain state synchronization.

## Setup

### Setup keys

```shell
# Example rich account from zksync os local networks.
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3
```

### Deploy WhitelistMirror on destination chain (3051)

```shell
forge create examples/03_whitelist/WhitelistMirror.sol:WhitelistMirror \
  -r http://localhost:3051 \
  --private-key $PRIVATE_KEY \
  --broadcast

# Deployed to: 0x....
export MIRROR_ADDR=0x....
```

Sanity check:

```shell
cast call -r http://localhost:3051 $MIRROR_ADDR "trustedSenderHash()(bytes32)"
# 0x0000...0000
```

### Compute destination recipient bytes (ERC-7930)
We want the source contract to send messages to (destinationChainId, MIRROR_ADDR).

First, get destination chain id:

```shell
cast chain-id -r http://localhost:3051
# 6566 (example)
export DEST_CHAIN_ID=6566
```

Now compute ERC-7930 bytes for chainId + address:

```shell
cargo run encode 7930 --chain-id $DEST_CHAIN_ID --address $MIRROR_ADDR
# 0x...
export DEST_RECIPIENT=0x...
```

### Deploy WhitelistSource on source chain (3050)
InteropCenter system contract address (from zkSync OS) is:

```shell
export INTEROP_CENTER=0x0000000000000000000000000000000000010010
```

Deploy:

```shell
forge create examples/03_whitelist/WhitelistSource.sol:WhitelistSource \
  -r http://localhost:3050 \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --constructor-args $INTEROP_CENTER $DEST_RECIPIENT

# Deployed to: 0x....
export SOURCE_ADDR=0x....
```

### Set trusted sender on WhitelistMirror (destination chain)
WhitelistMirror must only accept updates from the source contract.
Compute ERC-7930 bytes for `(sourceChainId, SOURCE_ADDR)`.

```shell
cast chain-id -r http://localhost:3050
# 6565 (example)
export SRC_CHAIN_ID=6565

cargo run encode 7930 --chain-id $SRC_CHAIN_ID --address $SOURCE_ADDR
# 0x...
export TRUSTED_SENDER=0x...
```

Set it on destination:

```shell
cast send -r http://localhost:3051 \
  --private-key $PRIVATE_KEY \
  $MIRROR_ADDR \
  "setTrustedSender(bytes)" \
  $TRUSTED_SENDER
```

Check:

```shell
cast call -r http://localhost:3051 $MIRROR_ADDR "trustedSenderHash()(bytes32)"
# now non-zero
```

## Send a whitelist update from source chain

Pick an address to whitelist:

```shell
export ACCOUNT=0x000000000000000000000000000000000000dEaD
```

Send `add(ACCOUNT)` on the source chain:

```shell
cast send -r http://localhost:3050 \
  --private-key $PRIVATE_KEY \
  $SOURCE_ADDR \
  "add(address)" \
  $ACCOUNT

# tx hash: 0x....
export WHITELIST_TX=0x....
```

Inspect the source tx (you should see an interop bundle/message being emitted):

```shell
cargo run debug tx --rpc http://localhost:3050 $WHITELIST_TX
```

### Relay to destination chain
Now relay the transaction from source to destination:

```shell
cargo run bundle relay \
  --rpc-src http://localhost:3050 \
  --rpc-dest http://localhost:3051 \
  --tx $WHITELIST_TX \
  --private-key $PRIVATE_KEY

# ... waits for proof/root ...
# sent tx: 0x....
export EXECUTE_TX=0x....
```

### Final checks
On the destination chain, the mirror should now show the account as whitelisted:

```shell
cast call -r http://localhost:3051 $MIRROR_ADDR "isWhitelisted(address)(bool)" $ACCOUNT
# true
```

You can also inspect the execute tx logs on destination:

```shell
cargo run debug tx --rpc http://localhost:3051 $EXECUTE_TX
# should show WhitelistUpdated(action=1, account=..., isWhitelistedNow=true)
```

### Debugging cheatsheet (basic)
If relay is stuck:

* Check the source tx events:

```shell
cargo run debug tx --rpc http://localhost:3050 $WHITELIST_TX
```

* Fetch proof:

```shell
cargo run debug proof --rpc http://localhost:3050 --tx $WHITELIST_TX
```

* Wait for root:

```shell
# fill in batch + root from debug proof output
cargo run debug root --rpc http://localhost:3051 --source-chain $SRC_CHAIN_ID --batch <BATCH> --expected-root <ROOT>
```

* Check bundle status on destination once you know the bundle hash:

```shell
cargo run bundle status --rpc http://localhost:3051 --bundle-hash <BUNDLE_HASH>
```