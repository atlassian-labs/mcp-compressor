use std::path::PathBuf;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{BackendServerConfig, CompressedServerConfig};

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn python_command() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
}

pub fn backend(name: &str, fixture: &str) -> BackendServerConfig {
    BackendServerConfig::new(
        name,
        python_command(),
        [fixture_path(fixture).to_string_lossy().into_owned()],
    )
}

pub fn max_config(server_name: impl Into<Option<&'static str>>) -> CompressedServerConfig {
    CompressedServerConfig {
        level: CompressionLevel::Max,
        server_name: server_name.into().map(str::to_string),
        include_tools: Vec::new(),
        exclude_tools: Vec::new(),
        toonify: false,
    }
}
