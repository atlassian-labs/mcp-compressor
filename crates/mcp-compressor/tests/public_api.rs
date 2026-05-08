use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{CompressorClient, CompressorMode, GeneratedClientKind, ServerConfig};

#[test]
fn public_crate_exports_expected_sdk_surface() {
    let _client = CompressorClient::builder()
        .server("alpha", ServerConfig::command("python").arg("alpha_server.py"))
        .compression_level(CompressionLevel::Max)
        .mode(CompressorMode::CompressedTools)
        .build();

    let _kind = GeneratedClientKind::Python;
}
