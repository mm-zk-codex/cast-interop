# Auto relay example

## Prerequisites: validate chain configuration

Before starting the auto-relayer, run:

```bash
cast-interop chains validate
```

**Why this matters for auto-relay specifically:**

The auto-relayer watches chains continuously and relays every `InteropBundleSent` event it sees.
If a chain alias has a stale chainId in the config (e.g. because the testnet was reset or a config
was copied from a different environment), the relayer will silently fail every single bundle it
tries to relay — with errors like `WrongDestinationChainId` that don't point at the config as the
cause. You could watch hundreds of jobs fail before realising the chainId was wrong from the start.

`chains validate` catches this before you start, along with two other silent failure modes:
- **RPC unreachable** — a chain that's down will block proof fetching for all bundles destined to it
- **Missing zkSync methods** — a generic public RPC node that doesn't support `zks_getL2ToL1LogProof`
  or `zks_getL1BatchNumber` will cause proof fetching to fail with network-looking errors

Example output (healthy):

```
chain: era
  ✅ rpc_reachable: RPC reachable
  ✅ chain_id_match: chainId 324 matches live RPC
  ✅ zks_log_proof: zks_getL2ToL1LogProof supported
  ✅ zks_batch_number: zks_getL1BatchNumber supported

chain: test
  ✅ rpc_reachable: RPC reachable
  ✅ chain_id_match: chainId 325 matches live RPC
  ✅ zks_log_proof: zks_getL2ToL1LogProof supported
  ✅ zks_batch_number: zks_getL1BatchNumber supported

2 chain(s) validated — 0 failure(s), 0 warning(s)
```

If any chain shows a `chain_id_match` failure, re-add it to refresh the stored chainId:

```bash
cast-interop chains rm <alias>
cast-interop chains add <alias> --rpc <URL>
cast-interop chains validate <alias>   # confirm fixed
```

---

## Start the auto-relay

Start the auto-relay UI (execute-only):

```bash
cast-interop auto-relay \
  --rpc http://localhost:3050 \
  --rpc http://localhost:3051 \
  --private-key $PRIVATE_KEY
```

Trigger a message (Greeting or Whitelist example):

```bash
# Example: send a greeting message on the source chain.
cast-interop send message --chain era --to-chain test --to 0xTARGET --payload 0xdeadbeef --private-key $PRIVATE_KEY
```

Watch the auto-relay UI detect the bundle, fetch proofs, wait for roots, and execute on the
destination chain.

Type `q` + enter to quit or `R` + enter to retry all failures.

## Failure + retry flow

If the destination rejects execution (for example, a Whitelist sender is not trusted), the job
enters `FAIL` with a short error message. Fix the destination configuration and press `R` in the
UI to requeue failed jobs.
