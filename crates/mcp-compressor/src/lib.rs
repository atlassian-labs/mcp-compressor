//! Public Rust SDK crate for mcp-compressor.
//!
//! This crate intentionally hides the internal `mcp-compressor-core` crate name
//! from end-user Rust code while re-exporting the public SDK/runtime surface.

pub use mcp_compressor_core::*;
