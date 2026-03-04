# Examples

This directory contains different examples of how interop can be used. All of them depend only on basic tools (forge, cast, cast-interop).

Assumptions for all examples:

* Run commands from the repository root.
* Local zkSync OS RPC endpoints are available (`http://localhost:3050` and `http://localhost:3051`).
* If using source execution, Rust/Cargo are installed.
* If using source execution, use `cargo run -- <command> ...` (note the `--`).
* If using a compiled binary, run `cargo build --release` first and replace `cargo run --` with `./target/release/cast-interop`.

* [Greeting](01_greeting/README.md) - basic example on how to send messages from one chain to another.
* [Token](02_token/README.md) - example on how to send tokens from one chain to another.
* [Whitelist](03_whitelist/README.md) - more complex example of message passing with some authorization

There is also a section about how to do more advanced debugging:

* [Debugging](10_debug_basics/basics.md) - Simple debugging
* [Debugging](10_debug_basics/retry.md) - How to handle retries etc.
