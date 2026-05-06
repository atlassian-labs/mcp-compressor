//! JSON-serializable DTOs and helpers for PyO3 / napi-rs language bindings.
//!
//! These are not a C ABI. They are intentionally plain Rust data-transfer
//! objects that binding crates can expose idiomatically in Python and
//! TypeScript while sharing the same core behavior.

pub mod client_gen;
pub mod dto;
pub mod oauth;
pub mod pure;
pub mod session;
#[cfg(test)]
mod types;

pub use client_gen::{generate_client_artifacts, FfiClientArtifactKind};
pub use dto::{
    FfiBackendConfig, FfiCompressedSessionConfig, FfiCompressedSessionInfo,
    FfiGeneratorConfig, FfiJustBashCommandSpec, FfiJustBashProviderSpec, FfiMcpServer, FfiTool,
};
pub use oauth::{
    clear_oauth_credentials, list_oauth_credentials, oauth_store_path, remember_oauth_backend,
    FfiOAuthStoreEntry,
};
pub use pure::{
    compress_tool_listing, format_tool_schema_response, parse_mcp_config, parse_tool_argv,
};
pub use session::{
    start_compressed_session, start_compressed_session_from_mcp_config, FfiCompressedSession,
};
