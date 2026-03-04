# Auto relay example

## Prerequisites: validate chain configuration

Before starting the auto-relayer, verify that every configured chain alias has a reachable RPC
and a correct chainId. The auto-relayer runs continuously and silently skips bundles it cannot
relay — a misconfigured chainId is especially dangerous because it causes every relay attempt to
fail with confusing errors rather than a clear misconfiguration message.

```bash
cast-interop chains validate
```

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

If any chain shows a `chain_id_match` failure, re-add it:

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
