# Auto relay example

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
