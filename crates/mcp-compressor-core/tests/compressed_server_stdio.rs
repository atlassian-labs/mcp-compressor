mod common;

use mcp_compressor_core::server::CompressedServer;
use serde_json::json;

#[tokio::test]
async fn single_stdio_backend_exposes_only_compressed_wrapper_tools() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
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
async fn single_stdio_backend_schema_listing_invocation_resources_and_prompts_work() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let schema = server
        .get_tool_schema("alpha_get_tool_schema", "echo")
        .await
        .unwrap();
    assert!(schema.contains("echo"));
    assert!(schema.contains("message"));

    let listed = server.list_backend_tools("alpha_list_tools").await.unwrap();
    assert!(listed.contains("echo"));
    assert!(listed.contains("add"));
    assert!(listed.contains("structured_data"));

    let echo = server
        .invoke_tool("alpha_invoke_tool", "echo", json!({ "message": "hello" }))
        .await
        .unwrap();
    assert_eq!(echo, "alpha:hello");

    let add = server
        .invoke_tool("alpha_invoke_tool", "add", json!({ "a": 2, "b": 5 }))
        .await
        .unwrap();
    assert_eq!(add, "7");

    let resources = server.list_resources().await.unwrap();
    assert!(resources
        .iter()
        .any(|uri| uri == "fixture://alpha-resource"));
    assert!(resources
        .iter()
        .any(|uri| uri == "compressor://alpha/uncompressed-tools"));
    assert_eq!(
        server
            .read_resource("fixture://alpha-resource")
            .await
            .unwrap(),
        "alpha resource"
    );

    let prompts = server.list_prompts().await.unwrap();
    assert!(prompts.iter().any(|name| name == "alpha_prompt"));
}
