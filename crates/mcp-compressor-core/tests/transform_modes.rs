mod common;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{
    BackendConfigSource, CompressedServer, CompressedServerConfig, ProxyTransformMode,
};
use serde_json::json;

#[tokio::test]
async fn compressed_tools_mode_exposes_wrappers_for_single_server() {
    let server = CompressedServer::connect_stdio(
        common::config(
            CompressionLevel::Max,
            Some("alpha"),
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::Command,
        ),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let names: Vec<String> = server
        .list_frontend_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(
        names,
        [
            "alpha_get_tool_schema",
            "alpha_invoke_tool",
            "alpha_list_tools"
        ]
    );
}

#[tokio::test]
async fn cli_mode_exposes_help_tool_and_keeps_exec_routing_available() {
    let server = CompressedServer::connect_stdio(
        common::config(
            CompressionLevel::Low,
            Some("alpha"),
            ProxyTransformMode::Cli,
            BackendConfigSource::Command,
        ),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let tools = server.list_frontend_tools().await.unwrap();
    let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    assert_eq!(names, ["alpha_help"]);
    assert!(tools[0]
        .description
        .as_deref()
        .unwrap_or_default()
        .contains("echo"));

    let result = server
        .invoke_tool("alpha_invoke_tool", "echo", json!({ "message": "hello" }))
        .await
        .unwrap();
    assert_eq!(result, "alpha:hello");
}

#[tokio::test]
async fn just_bash_mode_exposes_bash_tool_and_per_server_help_tools() {
    let server = CompressedServer::connect_multi_stdio(
        common::config(
            CompressionLevel::Low,
            None,
            ProxyTransformMode::JustBash,
            BackendConfigSource::Command,
        ),
        vec![
            common::backend("alpha", "alpha_server.py"),
            common::backend("beta", "beta_server.py"),
        ],
    )
    .await
    .unwrap();

    let tools = server.list_frontend_tools().await.unwrap();
    let names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    assert_eq!(names, ["bash_tool", "alpha_help", "beta_help"]);

    let bash_description = tools
        .iter()
        .find(|tool| tool.name == "bash_tool")
        .and_then(|tool| tool.description.as_deref())
        .unwrap_or_default();
    assert!(bash_description.contains("just-bash"));
    assert!(bash_description.contains("alpha"));
    assert!(bash_description.contains("language-hosted just-bash"));
    assert!(bash_description.contains("beta"));
    assert!(bash_description.contains("TOON"));

    let specs = server.just_bash_provider_specs();
    assert_eq!(specs.len(), 2);
    let alpha = specs
        .iter()
        .find(|spec| spec.provider_name == "alpha")
        .unwrap();
    assert_eq!(alpha.help_tool_name, "alpha_help");
    let echo = alpha
        .tools
        .iter()
        .find(|command| command.backend_tool_name == "echo")
        .unwrap();
    assert_eq!(echo.command_name, "echo");
    assert_eq!(echo.invoke_tool_name, "alpha_invoke_tool");
    assert!(echo
        .description
        .as_deref()
        .unwrap_or_default()
        .contains("alpha"));
}

#[tokio::test]
async fn toonify_uses_official_toon_encoder_for_json_outputs() {
    let config = CompressedServerConfig {
        toonify: true,
        ..common::config(
            CompressionLevel::Low,
            Some("alpha"),
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::Command,
        )
    };
    let server =
        CompressedServer::connect_stdio(config, common::backend("alpha", "alpha_server.py"))
            .await
            .unwrap();

    let output = server
        .invoke_tool("alpha_invoke_tool", "structured_data", json!({}))
        .await
        .unwrap();
    assert!(output.contains("server: alpha"));
    assert!(output.contains("values"));
}
