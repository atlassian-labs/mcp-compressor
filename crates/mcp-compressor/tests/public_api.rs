use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{CompressorClient, CompressorMode, GeneratedClientKind, ServerConfig};
use std::collections::HashMap;

#[test]
fn public_crate_exports_expected_sdk_surface() {
    let _client = CompressorClient::builder()
        .server(
            "alpha",
            ServerConfig::command("python").arg("alpha_server.py"),
        )
        .compression_level(CompressionLevel::Max)
        .mode(CompressorMode::CompressedTools)
        .build();

    let _kind = GeneratedClientKind::Python;
}

#[test]
fn topology_server_config_preserves_public_fields() {
    let config = mcp_compressor::config::topology::ServerConfig {
        command: "python".to_string(),
        args: vec!["server.py".to_string()],
        env: HashMap::new(),
    };

    assert_eq!(config.command, "python");
}
