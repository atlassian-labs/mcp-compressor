use mcp_compressor::compression::CompressionLevel;
use mcp_compressor::sdk::{CompressorClient, ServerConfig};
use serde_json::json;

fn fixture_path(name: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    root.join("crates")
        .join("mcp-compressor-core")
        .join("tests")
        .join("fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn python_command() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
}

#[tokio::test]
async fn public_rust_sdk_quickstart_flow() {
    let client = CompressorClient::builder()
        .server(
            "alpha",
            ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
        )
        .compression_level(CompressionLevel::Medium)
        .build();

    let proxy = client.connect().await.unwrap();
    let tool_names: Vec<_> = proxy.tools().iter().map(|tool| tool.name.as_str()).collect();
    assert!(tool_names.contains(&"alpha_get_tool_schema"));
    assert!(tool_names.contains(&"alpha_invoke_tool"));

    let response = proxy.invoke("echo", json!({ "message": "public-rust" })).await.unwrap();
    assert_eq!(response, "alpha:public-rust");
}
