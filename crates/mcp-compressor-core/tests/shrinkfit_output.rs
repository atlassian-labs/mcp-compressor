mod common;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{BackendConfigSource, CompressedServer, ProxyTransformMode};
use serde_json::{json, Value};

#[tokio::test]
async fn cli_and_just_bash_return_recoverable_previews_and_keep_small_results() {
    for mode in [ProxyTransformMode::Cli, ProxyTransformMode::JustBash] {
        let mut config = common::config(
            CompressionLevel::Low,
            Some("alpha"),
            mode,
            BackendConfigSource::Command,
        );
        config.shrinkfit = Some(2000);
        let server =
            CompressedServer::connect_stdio(config, common::backend("alpha", "alpha_server.py"))
                .await
                .unwrap();
        let message = "section: useful details\n".repeat(1000);
        let text = if mode == ProxyTransformMode::Cli {
            server
                .invoke_single_backend_tool("echo", json!({"message":message}))
                .await
                .unwrap()
        } else {
            server
                .invoke_tool("alpha_invoke_tool", "echo", json!({"message":message}))
                .await
                .unwrap()
        };
        assert!(text.chars().count() <= 2000);
        let output: Value = serde_json::from_str(&text).unwrap();
        let path = output["recovery"]["original_response_file"]
            .as_str()
            .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["content"][0]["text"], format!("alpha:{message}"));
        assert_eq!(output["recovery"]["fits"], true);
        assert_eq!(
            server
                .invoke_single_backend_tool("echo", json!({"message":"small"}))
                .await
                .unwrap(),
            "alpha:small"
        );
        let error = server
            .invoke_single_backend_tool("tool_error", json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("fixture tool error"));
        server.shutdown().await.unwrap();
        // Originals remain available after the backend and proxy are gone.
        assert!(std::path::Path::new(path).exists());
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn generated_cli_exposes_recovery_file() {
    use mcp_compressor_core::client_gen::cli::CliGenerator;
    use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
    use mcp_compressor_core::proxy::ToolProxyServer;
    let directory = tempfile::tempdir().unwrap();
    let mut config = common::config(
        CompressionLevel::Low,
        Some("alpha"),
        ProxyTransformMode::Cli,
        BackendConfigSource::Command,
    );
    config.shrinkfit = Some(2000);
    let server =
        CompressedServer::connect_stdio(config, common::backend("alpha", "alpha_server.py"))
            .await
            .unwrap();
    let tools = server.backend_tools();
    let proxy = ToolProxyServer::start(server).await.unwrap();
    let generator = GeneratorConfig {
        cli_name: "alpha".to_string(),
        bridge_url: proxy.bridge_url().to_string(),
        token: proxy.token_value().to_string(),
        tools,
        session_pid: std::process::id(),
        output_dir: directory.path().to_path_buf(),
        extra_cli_bridges: Vec::new(),
    };
    CliGenerator.generate(&generator).unwrap();
    let script = directory
        .path()
        .join(if cfg!(windows) { "alpha.cmd" } else { "alpha" });
    let message = "full original ".repeat(1000);
    let expected = format!("alpha:{message}");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(script)
            .args(["echo", "--message", &message])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = envelope["recovery"]["original_response_file"]
        .as_str()
        .unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["content"][0]["text"], expected);
    std::fs::remove_file(path).unwrap();
    proxy.shutdown().await.unwrap();
}

#[tokio::test]
async fn disabled_mode_keeps_large_response_verbatim() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let message = "exact original ".repeat(2000);
    let output = server
        .invoke_single_backend_tool("echo", json!({"message":message}))
        .await
        .unwrap();
    assert_eq!(output, format!("alpha:{message}"));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_sdk_config_fails_before_connecting() {
    let mut config = common::max_config(Some("alpha"));
    config.shrinkfit = Some(2000);
    let error = CompressedServer::connect_stdio(config, common::backend("alpha", "nonexistent.py"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("requires CLI/Just Bash"));
}
