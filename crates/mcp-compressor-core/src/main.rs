//! CLI entrypoint for the standalone Rust mcp-compressor core binary.
//!
//! This binary is intentionally present before the runtime implementation so
//! executable-level e2e contracts can compile and describe the intended CLI
//! surface. Runtime behavior remains TODO until the Rust core server/proxy is
//! implemented.

use std::process::ExitCode;

const HELP: &str = "mcp-compressor-core\n\nUSAGE:\n    mcp-compressor-core [OPTIONS] [-- <COMMAND>...]\n\nOPTIONS:\n    --help                      Print help\n    --compression <LEVEL>       low | medium | high | max\n    --config <PATH>             MCP config JSON file\n    --server-name <NAME>        Frontend server name/prefix\n    --transport <TYPE>          stdio | streamable-http\n    --transform-mode <MODE>     compressed-tools | cli | just-bash\n    --cli-mode                  Alias for --transform-mode cli\n    --just-bash                 Alias for --transform-mode just-bash\n";

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }

    if let Err(message) = validate_args(&args) {
        eprintln!("error: {message}\n\n{HELP}");
        return ExitCode::from(2);
    }

    todo!("standalone Rust CLI runtime is not implemented yet")
}

fn validate_args(args: &[String]) -> Result<(), String> {
    let mut passthrough = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if passthrough {
            index += 1;
            continue;
        }

        match arg.as_str() {
            "--" => {
                passthrough = true;
                index += 1;
            }
            "--compression" => {
                let level = args
                    .get(index + 1)
                    .ok_or_else(|| "--compression requires a value".to_string())?;
                match level.as_str() {
                    "low" | "medium" | "high" | "max" => {}
                    _ => return Err(format!("unknown compression level: {level}")),
                }
                index += 2;
            }
            "--config" | "--server-name" | "--transport" | "--transform-mode" => {
                if args.get(index + 1).is_none() {
                    return Err(format!("{arg} requires a value"));
                }
                index += 2;
            }
            "--cli-mode" | "--just-bash" => {
                index += 1;
            }
            option if option.starts_with('-') => return Err(format!("unknown option: {option}")),
            _ => {
                passthrough = true;
                index += 1;
            }
        }
    }

    Ok(())
}
