mod common;

use std::time::Duration;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{BackendConfigSource, CompressedServer, ProxyTransformMode};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

#[tokio::test]
async fn remote_json_mcp_config_uses_url_and_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let request_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        String::from_utf8(request).unwrap()
    });
    let config_json = json!({
        "mcpServers": {
            "remote": {
                "url": format!("http://{address}/mcp"),
                "headers": { "Authorization": "Bearer test-token" }
            }
        }
    })
    .to_string();

    let result = CompressedServer::connect_mcp_config_json(
        common::config(
            CompressionLevel::Max,
            None,
            ProxyTransformMode::CompressedTools,
            BackendConfigSource::SingleServerJsonConfig,
        ),
        &config_json,
    )
    .await;
    let error = match result {
        Ok(_) => panic!("401 fixture should reject the remote backend"),
        Err(error) => error,
    };
    assert!(
        !error.to_string().contains("missing field `command`"),
        "remote URL configuration must not require a command: {error}"
    );
    let request = tokio::time::timeout(Duration::from_secs(5), request_task)
        .await
        .expect("remote backend should connect to the loopback fixture")
        .unwrap();

    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-token"),
        "remote backend must forward configured headers"
    );
    assert!(
        error
            .to_string()
            .contains("failed to initialize remote streamable HTTP backend"),
        "remote URL configuration must use streamable HTTP: {error}"
    );
}
