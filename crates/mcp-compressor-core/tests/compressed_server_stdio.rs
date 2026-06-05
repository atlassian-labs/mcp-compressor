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
async fn single_stdio_backend_invoke_wrapper_tool_input_schema_is_explicit_open_object() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let tools = server.list_frontend_tools().await.unwrap();
    let invoke_tool = tools
        .iter()
        .find(|tool| tool.name == "alpha_invoke_tool")
        .unwrap();

    assert_eq!(
        invoke_tool
            .input_schema
            .pointer("/properties/tool_input")
            .unwrap(),
        &json!({
            "type": "object",
            "description": "JSON input for the backend tool. Use this when your tool-calling API preserves nested object properties.",
            "properties": {},
            "additionalProperties": true
        })
    );
    assert_eq!(
        invoke_tool
            .input_schema
            .pointer("/properties/tool_input_json")
            .unwrap(),
        &json!({
            "type": "string",
            "description": "JSON-serialized input object for the backend tool. Use this instead of tool_input if your tool-calling API drops nested object properties."
        })
    );
    assert_eq!(
        invoke_tool.input_schema.pointer("/required").unwrap(),
        &json!(["tool_name"])
    );
    assert!(invoke_tool
        .description
        .as_deref()
        .unwrap_or_default()
        .contains("tool_input_json"));
}

#[tokio::test]
async fn single_stdio_backend_accepts_tool_input_json_escape_hatch() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let echo = server
        .invoke_tool(
            "alpha_invoke_tool",
            "echo",
            json!({ "message": "json-string" }),
        )
        .await
        .unwrap();
    assert_eq!(echo, "alpha:json-string");
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
