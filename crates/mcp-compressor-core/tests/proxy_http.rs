mod common;

use std::io::{Read, Write};
use std::net::TcpStream;

use mcp_compressor_core::proxy::ToolProxyServer;
use mcp_compressor_core::server::CompressedServer;
use serde_json::json;

struct HttpResponse {
    status: u16,
    body: String,
}

fn split_http_url(url: &str) -> (&str, &str) {
    let rest = url
        .strip_prefix("http://")
        .expect("fixture proxy URL should be plain HTTP");
    rest.split_once('/')
        .map_or((rest, "/"), |(host, path)| (host, path))
}

fn send_raw_http(method: &str, url: &str, auth: Option<&str>, body: Option<&str>) -> HttpResponse {
    let (host, path) = split_http_url(url);
    let path = format!("/{path}");
    let body = body.unwrap_or("");

    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(token) = auth {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if !body.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);

    let mut stream = TcpStream::connect(host).unwrap();
    stream.write_all(request.as_bytes()).unwrap();

    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .expect("HTTP response should contain a numeric status code");

    HttpResponse {
        status,
        body: body.to_string(),
    }
}

#[tokio::test]
async fn proxy_health_is_public_and_exec_requires_bearer_token() {
    let compressed = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let proxy = ToolProxyServer::start(compressed).await.unwrap();

    let health = send_raw_http("GET", &proxy.health_url(), None, None);
    assert!((200..300).contains(&health.status));

    let body = json!({
        "tool": "alpha_invoke_tool",
        "input": { "tool_name": "echo", "tool_input": { "message": "hello" } }
    })
    .to_string();
    let missing_auth = send_raw_http("POST", &proxy.exec_url(), None, Some(&body));
    assert_eq!(missing_auth.status, 401);

    let wrong_auth = send_raw_http("POST", &proxy.exec_url(), Some("wrong-token"), Some(&body));
    assert_eq!(wrong_auth.status, 401);
}

#[tokio::test]
async fn proxy_exec_dispatches_to_real_backend_with_session_token() {
    let compressed = CompressedServer::connect_stdio(
        common::max_config(Some("alpha")),
        common::backend("alpha", "alpha_server.py"),
    )
    .await
    .unwrap();
    let proxy = ToolProxyServer::start(compressed).await.unwrap();

    let body = json!({
        "tool": "alpha_invoke_tool",
        "input": { "tool_name": "echo", "tool_input": { "message": "hello" } }
    })
    .to_string();
    let response = send_raw_http(
        "POST",
        &proxy.exec_url(),
        Some(proxy.token_value()),
        Some(&body),
    );

    assert!((200..300).contains(&response.status));
    assert_eq!(response.body.trim(), "alpha:hello");
}
