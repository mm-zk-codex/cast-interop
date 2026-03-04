# DEV

## Overview

`cast-interop` is a single-crate Rust CLI for zkSync interop workflows. The project now also
includes a Docker-based E2E harness for local developer testing.

## Project Structure

- `src/main.rs`: binary entrypoint, logging, CLI dispatch.
- `src/cli.rs`: clap command tree and argument definitions.
- `src/config.rs`: config loading and RPC resolution.
- `src/rpc.rs`: RPC helpers for standard Ethereum and zkSync-specific calls.
- `src/relay_flow.rs`: shared relay orchestration.
- `src/commands/`: one file per command handler.
- `deps/`: ABI artifacts used at runtime.
- `examples/`: manual workflow examples.
- `tests/common/mod.rs`: E2E test helpers for env parsing, subprocess execution, RPC checks, and
  the `Greet` fixture deployment/read helpers.
- `tests/bundle_relay.rs`: first Docker-driven E2E coverage for `bundle relay`.
- `scripts/e2e/docker-compose.yml`: compose template for Anvil plus two `zksync-os-server` chains.
- `scripts/test-e2e.sh`: harness that pulls the pinned upstream artifact, materializes configs,
  starts compose, runs the ignored integration tests, and stores logs.

## E2E Harness

The E2E harness is local-first and CI-friendly:

- Pins `zksync-os-server` to `v0.16.0`.
- Uses `ghcr.io/matter-labs/zksync-os-server:v0.16.0` as the default server image.
- Uses `ghcr.io/foundry-rs/foundry:v1.5.1` as the default Anvil image because the upstream
  `l1-state.json` format is documented against Foundry 1.5.1.
- Tries to extract `local-chains` assets from the image first.
- Falls back to the tagged source archive if the image does not contain the required
  `v31.0/multi_chain` fixtures.
- Starts three compose services: Anvil on `8545`, chain A on `3050`, chain B on `3051`.

The current flow coverage is intentionally narrow but now includes a real happy path:

- a Docker boot + JSON output smoke check
- a successful `bundle relay` using the checked-in `deps/Greet.json` artifact as the destination
  fixture on chain B
- a controlled `bundle relay` failure (`missing_receipt`)
