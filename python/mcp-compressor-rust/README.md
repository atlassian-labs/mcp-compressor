# mcp-compressor-rust

Experimental Rust-backed Python package for `mcp-compressor`.

This package is intentionally separate from the legacy `mcp_compressor` package while the Rust core migration is validated. It exposes thin Python wrappers around the `_mcp_compressor_core` native extension built from the Rust workspace.
