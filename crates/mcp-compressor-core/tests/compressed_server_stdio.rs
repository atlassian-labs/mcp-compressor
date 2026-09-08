mod common;

use mcp_compressor_core::{
    server::{registration::FrontendServer, CompressedServer},
    Error,
};
use rmcp::{
    model::{CallToolRequestParams, Meta},
    ServiceExt,
};
use serde_json::json;

#[tokio::test]
async fn mcp_frontend_preserves_complete_backend_tool_results() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        FrontendServer::new(server)
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let mut client = ().serve(client_transport).await.unwrap();

    let mut request = CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
        json!({"tool_name": "rich_result", "tool_input": {}})
            .as_object()
            .unwrap()
            .clone(),
    );
    request.meta = Some(Meta(
        json!({"trace": "forwarded"}).as_object().unwrap().clone(),
    ));
    let result = client.call_tool(request).await.unwrap();
    let mut result = serde_json::to_value(result).unwrap();
    let progress_token = result
        .pointer_mut("/_meta/request/progressToken")
        .expect("request metadata should reach the backend tool");
    assert!(progress_token.is_number());
    *progress_token = json!("forwarded");

    assert_eq!(
        result,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": "rich text",
                    "annotations": {"audience": ["assistant"], "priority": 0.75},
                    "_meta": {"content": "text"}
                },
                {
                    "type": "image",
                    "data": "aW1hZ2U=",
                    "mimeType": "image/png",
                    "annotations": {"audience": ["user"], "priority": 0.5},
                    "_meta": {"content": "image"}
                },
                {
                    "type": "audio",
                    "data": "YXVkaW8=",
                    "mimeType": "audio/wav",
                    "annotations": {"priority": 0.25}
                }
            ],
            "structuredContent": {"ok": true, "values": [1, 2]},
            "isError": false,
            "_meta": {
                "result": "rich",
                "request": {
                    "trace": "forwarded",
                    "progressToken": "forwarded"
                }
            }
        })
    );

    let error = client
        .call_tool(
            CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
                json!({"tool_name": "tool_error", "tool_input": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(error.is_error, Some(true));

    client.close().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_frontend_does_not_forward_the_caller_progress_token() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        FrontendServer::new(server)
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let mut client = ().serve(client_transport).await.unwrap();

    let mut request = CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
        json!({"tool_name": "rich_result", "tool_input": {}})
            .as_object()
            .unwrap()
            .clone(),
    );
    request.meta = Some(Meta(
        json!({"trace": "kept", "progressToken": "caller-token"})
            .as_object()
            .unwrap()
            .clone(),
    ));
    let result = client.call_tool(request).await.unwrap();
    let result = serde_json::to_value(result).unwrap();
    let backend_meta = result
        .pointer("/_meta/request")
        .expect("request metadata should reach the backend tool");

    // Unrelated caller metadata is forwarded, but the caller's progress token is
    // not: no progress notifications are relayed back, so honouring the token
    // would promise progress that never arrives.
    assert_eq!(backend_meta.get("trace"), Some(&json!("kept")));
    assert_ne!(
        backend_meta.get("progressToken"),
        Some(&json!("caller-token"))
    );

    client.close().await.unwrap();
    server_task.await.unwrap();
}

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
async fn unnamed_single_backend_rejects_unknown_wrapper_names() {
    let server = CompressedServer::connect_stdio(
        common::max_config(None),
        common::backend("", "alpha_server.py"),
    )
    .await
    .unwrap();

    let error = server
        .invoke_tool("unknown_invoke_tool", "add", json!({ "a": 2, "b": 5 }))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        Error::ToolNotFound(name) if name == "unknown_invoke_tool"
    ));
}

#[tokio::test]
async fn single_stdio_backend_invoke_wrapper_tool_input_schema_explains_selected_tool_schema() {
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
            "description": "JSON object matching the selected backend tool's input schema. Use get_tool_schema for the selected tool_name before invoking if required fields are unknown.",
            "properties": {},
            "additionalProperties": true
        })
    );
}

#[tokio::test]
async fn single_stdio_backend_rejects_empty_tool_input_for_required_backend_tool() {
    let server = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();

    let error = server
        .invoke_tool("alpha_invoke_tool", "echo", json!({}))
        .await
        .unwrap_err();
    let Error::Validation(message) = &error else {
        panic!("expected validation error, got {error:?}");
    };

    assert!(message.contains("echo"), "got: {message}");
    assert!(message.contains("message"), "got: {message}");
    assert!(message.contains("tool_input"), "got: {message}");
    assert!(message.contains("get_tool_schema"), "got: {message}");
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

/// `--toonify` must still shrink JSON payloads once the MCP frontend returns
/// backend results verbatim, and must not damage annotations or typed content.
#[tokio::test]
async fn mcp_frontend_toonifies_json_text_results() {
    let server = CompressedServer::connect_stdio(
        common::toonify_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        FrontendServer::new(server)
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let mut client = ().serve(client_transport).await.unwrap();

    let result = client
        .call_tool(
            CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
                json!({"tool_name": "json_rows", "tool_input": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let result = serde_json::to_value(result).unwrap();

    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("[2]{id,name}:"),
        "expected TOON table, got {text}"
    );
    assert!(text.contains("1,alpha"), "expected TOON rows, got {text}");
    assert_eq!(result["content"][0]["annotations"]["priority"], json!(0.75));

    client.close().await.unwrap();
    server_task.await.unwrap();
}
