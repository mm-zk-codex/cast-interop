use crate::abi::decode_calldata_bytes;
use crate::cli::DecodeCalldataArgs;
use crate::config::Config;
use crate::types::AddressBook;
use anyhow::Result;

/// Decode raw calldata or revert data offline against known interop selectors.
pub async fn run(args: DecodeCalldataArgs, _config: Config, _addresses: AddressBook) -> Result<()> {
    let hex_input = args.hex.trim_start_matches("0x");
    let data =
        hex::decode(hex_input).map_err(|e| anyhow::anyhow!("invalid hex input: {e}"))?;

    let decoded = decode_calldata_bytes(&data);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&decoded)?);
        return Ok(());
    }

    // Human-readable output
    let icon = match decoded.kind.as_str() {
        "function_call" => "📞",
        "error" => "❌",
        "bundle_struct" => "📦",
        _ => "❓",
    };

    println!("{icon} kind:     {}", decoded.kind);
    println!("   name:     {}", decoded.name);
    if !decoded.selector.is_empty() {
        println!("   selector: 0x{}", decoded.selector);
    }
    println!("   params:");
    let pretty = serde_json::to_string_pretty(&decoded.params)
        .unwrap_or_else(|_| decoded.params.to_string());
    for line in pretty.lines() {
        println!("     {line}");
    }

    Ok(())
}
