mod common;
use mcp_compressor_core::{
    compression::CompressionLevel,
    proxy::dispatch_exec,
    server::{registration::FrontendServer, CompressedServer},
};
use rmcp::{model::CallToolRequestParams, ServiceExt};
use serde_json::{json, Value};

async fn server() -> CompressedServer {
    let mut config = common::max_config(Some("recovery"));
    config.shrinkfit = Some(4000);
    CompressedServer::connect_stdio(config, common::backend("recovery", "recovery_server.py"))
        .await
        .unwrap()
}

#[tokio::test]
async fn schema_full_flag_is_opt_in_and_cache_does_not_repeat_backend_writes() {
    let server = server().await;
    let tools = server.list_frontend_tools().await.unwrap();
    let wrapper = tools
        .iter()
        .find(|t| t.name.ends_with("invoke_tool"))
        .unwrap();
    assert_eq!(
        wrapper.input_schema["properties"]["full"]["type"],
        "boolean"
    );
    assert!(wrapper.input_schema["properties"]["full"]["description"]
        .as_str()
        .unwrap()
        .contains("only when"));
    let args = json!({"marker":"original","full":true});
    let preview = server
        .invoke_tool(&wrapper.name, "records", args.clone())
        .await
        .unwrap();
    let preview: Value = serde_json::from_str(&preview).unwrap();
    assert!(preview.get("data").is_some());
    assert!(preview.get("preview").is_none());
    assert!(preview["truncation"]["message"]
        .as_str()
        .unwrap()
        .contains("full=true"));
    assert!(preview["truncation"]
        .get("original_response_file")
        .is_none());
    let full = dispatch_exec(
        &server,
        wrapper.name.clone(),
        json!({"tool_name":"records","tool_input":args,"full":true}),
    )
    .await
    .unwrap();
    let full: Value = serde_json::from_str(&full).unwrap();
    assert_eq!(full["calls"], 1);
    assert_eq!(full["backend_full"], true);
    assert!(full.get("truncation").is_none());
    assert_eq!(full["data"]["items"].as_array().unwrap().len(), 30);
    // Cache lookup never memoizes a normal invocation; repeat executes the backend.
    server
        .invoke_tool(
            &wrapper.name,
            "records",
            json!({"marker":"original","full":true}),
        )
        .await
        .unwrap();
    let full = server
        .invoke_tool_full(
            &wrapper.name,
            "records",
            json!({"marker":"original","full":true}),
            true,
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&full).unwrap()["calls"], 2);
    // Changed arguments miss and bypass pruning on that first call.
    let miss = server
        .invoke_tool_full(
            &wrapper.name,
            "records",
            json!({"marker":"different"}),
            true,
        )
        .await
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&miss).unwrap()["calls"], 3);
    assert!(!miss.contains("automatically shrunk"));
    assert!(dispatch_exec(
        &server,
        wrapper.name.clone(),
        json!({"tool_name":"records","tool_input":{},"full":"yes"})
    )
    .await
    .is_err());
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn mcp_transport_preserves_structured_output_and_recovers_complete_result() {
    let server = server().await;
    let wrapper = server
        .list_frontend_tools()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.name.ends_with("invoke_tool"))
        .unwrap()
        .name;
    let (a, b) = tokio::io::duplex(1 << 20);
    let task = tokio::spawn(async move { FrontendServer::new(server).serve(a).await.unwrap() });
    let client = ().serve(b).await.unwrap();
    let frontend = task.await.unwrap();
    let invoke = |full| {
        CallToolRequestParams::new(wrapper.clone()).with_arguments(
            json!({"tool_name":"records","tool_input":{"marker":"protocol"},"full":full})
                .as_object()
                .unwrap()
                .clone(),
        )
    };
    let shrunk = client.call_tool(invoke(false)).await.unwrap();
    assert!(shrunk
        .structured_content
        .as_ref()
        .unwrap()
        .get("truncation")
        .is_none());
    let text = serde_json::to_value(&shrunk).unwrap();
    let text: Value = serde_json::from_str(text["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(text.get("truncation").is_some());
    let full = client.call_tool(invoke(true)).await.unwrap();
    assert_eq!(shrunk.structured_content, full.structured_content);
    assert_eq!(full.structured_content.as_ref().unwrap()["calls"], 1);
    assert!(full
        .structured_content
        .as_ref()
        .unwrap()
        .get("truncation")
        .is_none());
    // Relevant request metadata is part of the cache key, not cross-context reuse.
    let mut changed = invoke(true);
    changed.meta = Some(rmcp::model::RequestMetaObject(rmcp::model::MetaObject(
        json!({"tenant":"different"}).as_object().unwrap().clone(),
    )));
    let changed = client.call_tool(changed).await.unwrap();
    assert_eq!(changed.structured_content.as_ref().unwrap()["calls"], 2);
    client.cancel().await.unwrap();
    frontend.cancel().await.unwrap();
}

#[tokio::test]
async fn disabled_shrinking_does_not_advertise_full() {
    for level in [
        CompressionLevel::Low,
        CompressionLevel::Medium,
        CompressionLevel::High,
        CompressionLevel::Max,
    ] {
        let mut config = common::max_config(Some("recovery"));
        config.level = level;
        let server = CompressedServer::connect_stdio(
            config,
            common::backend("recovery", "recovery_server.py"),
        )
        .await
        .unwrap();
        let wrapper = server
            .list_frontend_tools()
            .await
            .unwrap()
            .into_iter()
            .find(|t| t.name.ends_with("invoke_tool"))
            .unwrap();
        assert!(wrapper.input_schema["properties"].get("full").is_none());
        server.shutdown().await.unwrap();
    }
}
