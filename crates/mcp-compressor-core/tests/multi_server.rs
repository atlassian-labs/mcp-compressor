mod common;

use mcp_compressor_core::server::CompressedServer;
use serde_json::json;

#[tokio::test]
async fn multi_stdio_backends_are_prefixed_and_routed_independently() {
    let server = CompressedServer::connect_multi_stdio(
        common::max_config(Some("suite")),
        vec![
            common::backend("alpha", "alpha_server.py"),
            common::backend("beta", "beta_server.py"),
        ],
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

    for expected in [
        "suite_alpha_get_tool_schema",
        "suite_alpha_invoke_tool",
        "suite_alpha_list_tools",
        "suite_beta_get_tool_schema",
        "suite_beta_invoke_tool",
        "suite_beta_list_tools",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing frontend tool {expected}"
        );
    }

    let alpha_tools = server
        .list_backend_tools("suite_alpha_list_tools")
        .await
        .unwrap();
    let beta_tools = server
        .list_backend_tools("suite_beta_list_tools")
        .await
        .unwrap();
    assert!(alpha_tools.contains("add"));
    assert!(beta_tools.contains("multiply"));

    let alpha = server
        .invoke_tool("suite_alpha_invoke_tool", "add", json!({ "a": 3, "b": 7 }))
        .await
        .unwrap();
    let beta = server
        .invoke_tool(
            "suite_beta_invoke_tool",
            "multiply",
            json!({ "a": 4, "b": 5 }),
        )
        .await
        .unwrap();
    assert_eq!(alpha, "10");
    assert_eq!(beta, "20");

    let resources = server.list_resources().await.unwrap();
    assert!(resources
        .iter()
        .any(|uri| uri == "fixture://alpha-resource"));
    assert!(resources.iter().any(|uri| uri == "fixture://beta-resource"));
    assert!(resources
        .iter()
        .any(|uri| uri == "compressor://suite_alpha/uncompressed-tools"));
    assert!(resources
        .iter()
        .any(|uri| uri == "compressor://suite_beta/uncompressed-tools"));
}
