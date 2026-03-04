0p0# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is this?

`cast-interop` is a Rust CLI tool for ZKsync interop workflows. It helps extract bundles, fetch proofs, wait for roots, and execute/verify bundles across chains — like a specialized `cast` focused on cross-chain interop plumbing.

## Build & Run

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -- --help            # run via cargo
cargo clippy                   # lint
cargo fmt --check              # format check
cargo test                     # run unit tests (25 tests in src/abi.rs covering decode_calldata_bytes and proof verification)
```

The binary is `cast-interop` (defined in `Cargo.toml` as `[[bin]]`).

## Architecture

### Single-crate binary

This is a single Rust crate with no workspace. All code lives under `src/`.

### Module layout

- **`main.rs`** — Entry point. Initializes tracing (via `RUST_LOG` env filter) and dispatches to the CLI.
- **`cli.rs`** — clap-based CLI definition. Defines the top-level `Cli` struct and all subcommand enums (`Command`, `BundleSubcommand`, `DebugSubcommand`, etc.). Each subcommand's `run()` delegates to a handler in `commands/`.
- **`config.rs`** — TOML config loading from `~/.config/cast-interop/config.toml`. Defines `Config`, `ChainConfig`, `AddressConfig`. Handles chain alias resolution (`--rpc` vs `--chain`).
- **`rpc.rs`** — `RpcClient` wrapping an alloy `DynProvider` + reqwest `Client`. Provides helpers for `eth_call`, `raw_rpc` (JSON-RPC calls), log proof fetching, and finalization polling.
- **`signer.rs`** — Loads a `PrivateKeySigner` from `--private-key` flag, `--private-key-env` env var, or config.
- **`types.rs`** — Shared types (`AddressBook`, view structs for JSON output, Solidity struct definitions via `alloy_sol_types::sol!` for `InteropBundle`, `InteropCall`, `BundleAttributes`). Also defines default system contract addresses.
- **`abi.rs`** — ABI encoding/decoding helpers. Encodes contract calls (`verifyBundle`, `executeBundle`, `sendMessage`, `sendBundle`, `bundleStatus`, `interopRoots`) and decodes event data (`InteropBundleSent`, `MessageSent`). Contains the interop error selector map for revert decoding. Also implements `compute_leaf_hash`, `compute_merkle_root`, and `verify_proof_offline` for pure offline Merkle proof verification.
- **`encode.rs`** — ERC-7930 address encoding/decoding and interop attribute encoding (call value, indirect call, execution address, unbundler address, asset ID).
- **`relay_flow.rs`** — Core relay orchestration: `wait_for_root`, `wait_for_proof`, `build_message_proof`, `execute_bundle`. Coordinates the proof→root→execute pipeline.
- **`commands/`** — One file per CLI subcommand. Each exports a `run()` function taking clap args, `Config`, and `AddressBook`.

### Key patterns

- **alloy ecosystem**: Uses `alloy-primitives`, `alloy-sol-types`, `alloy-provider`, `alloy-signer-local` for all Ethereum types and interactions. Solidity structs/functions are defined inline with the `sol!` macro.
- **RPC dual approach**: alloy `Provider` for standard eth calls, raw reqwest for ZKsync-specific RPC methods (`zks_getL2ToL1LogProof`, etc.) via `raw_rpc()`.
- **AddressBook**: System contract addresses (InteropCenter `0x...10010`, InteropHandler `0x...1000d`, InteropRootStorage `0x...10008`) are resolved from config → CLI flags → hardcoded defaults.
- **`--json` flag**: Most commands support structured JSON output alongside human-readable output.
- **`--dry-run`**: Transaction-sending commands support dry-run simulation via `eth_call` instead of actual submission.

### External dependencies

- **`deps/`** — ABI JSON files (`InteropCenter.json`, `InteropHandler.json`, `MessageVerification.json`, `Greet.json`) used at runtime for dynamic ABI decoding.
- **`context/`** — Solidity source files for the interop system contracts (InteropCenter, InteropHandler, MessageVerification, Messaging libraries). Reference only — not compiled by this project.
- **`examples/`** — End-to-end usage examples (greeting, token bridging, whitelist, auto-relay).
