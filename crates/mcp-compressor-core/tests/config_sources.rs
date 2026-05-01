mod common;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{BackendConfigSource, CompressedServer, ProxyTransformMode};
use serde_json::json;

#[tokio::test]
async fn single_server_direct_command_config_connects_and_invokes() {
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

    let result = server
        .invoke_tool("alpha_invoke_tool", "add", json!({ "a": 2, "b": 5 }))
        .await
        .unwrap();
    assert_eq!(result, "7");
}

#[tokio::test]
async fn single_server_json_mcp_config_connects_and_invokes() {
    let config_json = common::mcp_config_json(&[("alpha", "alpha_server.py")]);
    let server = CompressedServer::connect_mcp_config_json(
        common::config(
            CompressionLevel::Max,
            None,
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::SingleServerJsonConfig,
        ),
        &config_json,
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
    assert_eq!(names, ["get_tool_schema", "invoke_tool", "list_tools"]);

    let result = server
        .invoke_tool("invoke_tool", "add", json!({ "a": 2, "b": 5 }))
        .await
        .unwrap();
    assert_eq!(result, "7");
}

#[tokio::test]
async fn multi_server_direct_command_config_connects_and_routes() {
    let server = CompressedServer::connect_multi_stdio(
        common::config(
            CompressionLevel::Max,
            Some("suite"),
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::Command,
        ),
        vec![
            common::backend("alpha", "alpha_server.py"),
            common::backend("beta", "beta_server.py"),
        ],
    )
    .await
    .unwrap();

    let alpha = server
        .invoke_tool("suite_alpha_invoke_tool", "add", json!({ "a": 3, "b": 7 }))
        .await
        .unwrap();
    let beta = server
        .invoke_tool("suite_beta_invoke_tool", "multiply", json!({ "a": 4, "b": 5 }))
        .await
        .unwrap();
    assert_eq!(alpha, "10");
    assert_eq!(beta, "20");
}

#[tokio::test]
async fn multi_server_json_mcp_config_connects_and_routes() {
    let config_json = common::mcp_config_json(&[
        ("alpha", "alpha_server.py"),
        ("beta", "beta_server.py"),
    ]);
    let server = CompressedServer::connect_mcp_config_json(
        common::config(
            CompressionLevel::Max,
            Some("suite"),
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::MultiServerJsonConfig,
        ),
        &config_json,
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
    assert!(names.iter().any(|name| name == "suite_alpha_invoke_tool"));
    assert!(names.iter().any(|name| name == "suite_beta_invoke_tool"));

    let alpha = server
        .invoke_tool("suite_alpha_invoke_tool", "add", json!({ "a": 3, "b": 7 }))
        .await
        .unwrap();
    let beta = server
        .invoke_tool("suite_beta_invoke_tool", "multiply", json!({ "a": 4, "b": 5 }))
        .await
        .unwrap();
    assert_eq!(alpha, "10");
    assert_eq!(beta, "20");
}
