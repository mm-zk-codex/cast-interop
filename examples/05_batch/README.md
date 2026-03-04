# Batch Send example

Send multiple interop messages from a single JSON file, with optional auto-relay.

Instead of running `send message` multiple times with shell wiring, `send batch` reads a batch file and sends each message in sequence.

## Prerequisites

- Local zkSync OS multi-chain running on ports 3050 (source) and 3051 (destination)
- A deployed contract on the destination chain (e.g., Greeting.sol from `01_greeting`)

## Batch file format

Create a `batch.json` file:

```json
{
  "messages": [
    {
      "toChain": "6566",
      "to": "0x163CFa0911B9C7166b2608F0E902Fcd341523552",
      "payload": "0x00000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000005hello000000000000000000000000000000000000000000000000000000000000"
    },
    {
      "toChain": "6566",
      "to": "0x163CFa0911B9C7166b2608F0E902Fcd341523552",
      "abi": "greet(string)",
      "args": ["world"]
    }
  ]
}
```

Each message supports either:
- **`payload`**: Raw hex-encoded calldata
- **`abi` + `args`**: Human-readable function signature with arguments (auto-encoded)

Optional per-message fields: `interopValue`, `indirect`, `executionAddress`, `unbundler`.

## Usage

### Send only (no relay)

```shell
export PRIVATE_KEY=0xac1e735be8536c6534bb4f17f06f6afc73b2b5ba84ac2cfb12f7461b20c0bbe3

cargo run send batch \
  --rpc http://localhost:3050 \
  --file examples/05_batch/batch.json \
  --private-key $PRIVATE_KEY
```

### Send with auto-relay

```shell
cargo run send batch \
  --rpc http://localhost:3050 \
  --file examples/05_batch/batch.json \
  --private-key $PRIVATE_KEY \
  --relay \
  --rpc-dest http://localhost:3051
```

Example output:

```
[1/2] tx: 0xabc...  relayed: 0xdef...
[2/2] tx: 0x123...  relayed: 0x456...

batch complete: 2/2 sent, 2 relayed
```

### Verify on destination

```shell
export CONTRACT_ADDR=0x163CFa0911B9C7166b2608F0E902Fcd341523552
cast call -r http://localhost:3051 $CONTRACT_ADDR "message()(string)"
# "world"
```

### JSON output

```shell
cargo run send batch \
  --rpc http://localhost:3050 \
  --file examples/05_batch/batch.json \
  --private-key $PRIVATE_KEY \
  --json
```

### Dry run

```shell
cargo run send batch \
  --rpc http://localhost:3050 \
  --file examples/05_batch/batch.json \
  --dry-run
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--rpc` / `--chain` | Source chain RPC | required |
| `--file` | Path to batch JSON file | required |
| `--private-key` | Private key hex | - |
| `--private-key-env` | Env var with private key | PRIVATE_KEY |
| `--dry-run` | Simulate without sending | false |
| `--json` | Emit JSON output | false |
| `--relay` | Auto-relay each message | false |
| `--rpc-dest` / `--chain-dest` | Destination chain for relay | - |
| `--timeout-ms` | Relay timeout | 300000 |
| `--poll-ms` | Relay poll interval | 1000 |
