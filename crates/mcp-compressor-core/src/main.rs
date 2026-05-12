//! CLI entrypoint for the standalone Rust mcp-compressor core binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    mcp_compressor_core::app::entrypoint::main_exit_code()
}
