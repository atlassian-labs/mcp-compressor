mod common;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{BackendConfigSource, CompressedServer, ProxyTransformMode};

#[tokio::test]
async fn low_medium_and_high_expose_only_schema_and_invoke_wrappers() {
    for level in [
        CompressionLevel::Low,
        CompressionLevel::Medium,
        CompressionLevel::High,
    ] {
        let server = CompressedServer::connect_stdio(
            common::config(
                level.clone(),
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
            ["alpha_get_tool_schema", "alpha_invoke_tool"],
            "unexpected wrapper tools for {level:?}"
        );
    }
}

#[tokio::test]
async fn max_exposes_schema_invoke_and_list_wrappers() {
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
async fn wrapper_get_schema_description_uses_level_specific_tool_listing() {
    let expectations = [
        (
            CompressionLevel::Low,
            "<tool>echo(message): Echo a message from alpha.</tool>",
        ),
        (
            CompressionLevel::Medium,
            "<tool>echo(message): Echo a message from alpha</tool>",
        ),
        (CompressionLevel::High, "<tool>echo(message)</tool>"),
        (CompressionLevel::Max, "<tool>echo</tool>"),
    ];

    for (level, expected_listing) in expectations {
        let server = CompressedServer::connect_stdio(
            common::config(
                level.clone(),
                Some("alpha"),
                ProxyTransformMode::CompressedTools,
                BackendConfigSource::Command,
            ),
            common::backend("alpha", "alpha_server.py"),
        )
        .await
        .unwrap();
        let tools = server.list_frontend_tools().await.unwrap();
        let get_schema = tools
            .iter()
            .find(|tool| tool.name == "alpha_get_tool_schema")
            .unwrap();
        let description = get_schema.description.as_deref().unwrap_or_default();
        assert!(
            description.contains("Get the input schema for a specific tool from the alpha toolset")
        );
        assert!(
            description.contains(expected_listing),
            "level {level:?} description did not include expected listing {expected_listing:?}: {description}"
        );
    }
}

#[tokio::test]
async fn all_compression_levels_can_fetch_schema_and_invoke_backend_tools() {
    for level in [
        CompressionLevel::Low,
        CompressionLevel::Medium,
        CompressionLevel::High,
        CompressionLevel::Max,
    ] {
        let server = CompressedServer::connect_stdio(
            common::config(
                level.clone(),
                Some("alpha"),
                ProxyTransformMode::CompressedTools,
                BackendConfigSource::Command,
            ),
            common::backend("alpha", "alpha_server.py"),
        )
        .await
        .unwrap();

        let schema = server
            .get_tool_schema("alpha_get_tool_schema", "echo")
            .await
            .unwrap();
        assert!(
            schema.contains("echo"),
            "schema missing tool name for {level:?}"
        );
        assert!(
            schema.contains("message"),
            "schema missing input field for {level:?}"
        );

        let result = server
            .invoke_tool(
                "alpha_invoke_tool",
                "echo",
                serde_json::json!({ "message": "hello" }),
            )
            .await
            .unwrap();
        assert_eq!(result, "alpha:hello", "invoke failed for {level:?}");
    }
}
