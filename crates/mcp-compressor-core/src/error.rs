//! Crate-wide error type.

/// All errors produced by the mcp-compressor-core crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown compression level: {0:?}")]
    UnknownCompressionLevel(String),

    #[error("tool not found: {0:?}")]
    ToolNotFound(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("LLM assist error: {0}")]
    LlmAssist(String),
}
