//! Shared Rust core for mcp-compressor.
//!
//! # Module layout
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`compression`] | Pure tool-listing formatter and schema lookup |
//! | [`config`] | MCP config JSON parsing, server naming |
//! | [`proxy`] | Generic HTTP tool proxy with bearer-token auth |
//! | [`client_gen`] | Artifact generators (shell, Python, TypeScript) |
//! | [`cli`] | CLI name mapping and argv → tool-input parsing |
//! | [`server`] | `CompressedServer`, `ToolCache`, tool registration |
//! | [`ffi`] | FFI-safe surface for PyO3 / napi-rs language bindings |

pub mod cli;
pub mod client_gen;
pub mod compression;
pub mod config;
pub mod error;
pub mod ffi;
pub mod proxy;
pub mod server;

pub use error::Error;
