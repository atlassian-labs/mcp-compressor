mod common;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::post,
};
use mcp_compressor_core::{
    Error,
    server::{
        BackendAuthMode, BackendServerConfig, CompressedServer, registration::FrontendServer,
    },
};
use rmcp::{
    model::{CallToolRequestParams, MetaObject, RequestMetaObject},
    service::{ClientLifecycleMode, ClientServiceExt},
    ServiceExt,
};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::task::JoinSet;

#[derive(Clone)]
struct HangingHttpState {
    call_started: Arc<AtomicBool>,
    call_dropped: Arc<AtomicBool>,
    call_cancelled: Arc<AtomicBool>,
}

struct MarkDropped(Arc<AtomicBool>);

impl Drop for MarkDropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn hanging_http_mcp(
    State(state): State<HangingHttpState>,
    Json(request): Json<serde_json::Value>,
) -> Response<Body> {
    let method = request["method"].as_str().unwrap_or_default();
    let id = request.get("id").cloned().unwrap_or(json!(null));
    match method {
        "initialize" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": request["params"]["protocolVersion"],
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "hanging-http", "version": "1.0.0" }
            }
        }))
        .into_response(),
        "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
        "notifications/cancelled" => {
            state.call_cancelled.store(true, Ordering::SeqCst);
            StatusCode::ACCEPTED.into_response()
        }
        "tools/list" => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
                    "name": "hang",
                    "description": "Never responds",
                    "inputSchema": { "type": "object", "properties": {} }
                }]
            }
        }))
        .into_response(),
        "tools/call" => {
            state.call_started.store(true, Ordering::SeqCst);
            let _mark_dropped = MarkDropped(state.call_dropped.clone());
            std::future::pending().await
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" }
        }))
        .into_response(),
    }
}

fn assert_backend_timeout(error: &Error, backend: &str, operation: &str) {
    let Error::Config(message) = error else {
        panic!("expected a backend timeout configuration error, got: {error}");
    };
    assert_eq!(
        message,
        &format!(
            "backend {backend:?} timed out during {operation} after {:?}",
            Duration::from_millis(500)
        )
    );
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    use std::ffi::c_void;

    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        false
    } else {
        unsafe { CloseHandle(handle) };
        true
    }
}

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
    let mut client =
        ().serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
            .await
            .unwrap();

    let mut request = CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
        json!({"tool_name": "rich_result", "tool_input": {}})
            .as_object()
            .unwrap()
            .clone(),
    );
    request.meta = Some(RequestMetaObject(MetaObject(
        json!({"trace": "forwarded"}).as_object().unwrap().clone(),
    )));
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
    let mut client =
        ().serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
            .await
            .unwrap();

    let mut request = CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
        json!({"tool_name": "rich_result", "tool_input": {}})
            .as_object()
            .unwrap()
            .clone(),
    );
    request.meta = Some(RequestMetaObject(MetaObject(
        json!({"trace": "kept", "progressToken": "caller-token"})
            .as_object()
            .unwrap()
            .clone(),
    )));
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
    assert!(
        resources
            .iter()
            .any(|uri| uri == "fixture://alpha-resource")
    );
    assert!(
        resources
            .iter()
            .any(|uri| uri == "compressor://alpha/uncompressed-tools")
    );
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
    let mut client =
        ().serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
            .await
            .unwrap();

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

    // CSV text blocks take the same route, so one more call covers them.
    let result = client
        .call_tool(
            CallToolRequestParams::new("alpha_invoke_tool").with_arguments(
                json!({"tool_name": "csv_rows", "tool_input": {}})
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

    client.close().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn mcp_frontend_identifies_itself_as_mcp_compressor() {
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
    let mut client =
        ().serve_with_lifecycle(client_transport, ClientLifecycleMode::Initialize)
            .await
            .unwrap();

    // Not rmcp's default identity: clients log and display this name.
    let peer = client.peer_info().unwrap();
    let info = peer.server_info.clone().unwrap();
    assert_eq!(info.name, "mcp-compressor");
    assert_eq!(info.version, mcp_compressor_core::product_version());
    drop(peer);

    client.close().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn configured_timeout_bounds_connection_and_cleans_up_child() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("hanging.pid");
    let ready_file = temp.path().join("ready");
    let timeout = Duration::from_millis(500);
    let backend = common::backend("hanging", "hanging_server.py")
        .with_env([
            ("HANG_OPERATION", "connection"),
            ("PID_FILE", pid_file.to_str().unwrap()),
            ("READY_FILE", ready_file.to_str().unwrap()),
            ("STARTUP_DELAY", "0.6"),
        ])
        .with_timeout(timeout);

    let result = common::expire_after_fixture_ready(
        CompressedServer::connect_stdio(common::max_config(Some("hanging")), backend),
        &ready_file,
        "connection",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "connection");

    let pid = std::fs::read_to_string(pid_file)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    common::drive_with_frozen_time(async {
        while process_exists(pid) {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test]
async fn zero_backend_timeout_is_rejected_before_spawn() {
    let backend = common::backend("hanging", "hanging_server.py").with_timeout(Duration::ZERO);

    let error = CompressedServer::connect_stdio(common::max_config(Some("hanging")), backend)
        .await
        .unwrap_err();

    let Error::Validation(message) = error else {
        panic!("expected validation error, got {error:?}");
    };
    assert!(message.contains("hanging"), "got: {message}");
    assert!(message.contains("greater than zero"), "got: {message}");
}

#[tokio::test(start_paused = true)]
async fn single_mcp_config_timeout_names_configured_backend() {
    let temp = tempfile::tempdir().unwrap();
    let ready_file = temp.path().join("ready");
    let config = json!({
        "mcpServers": {
            "slow": {
                "command": common::python_command(),
                "args": [
                    common::fixture_path("hanging_server.py"),
                    "--timeout",
                    "0.5"
                ],
                "env": {
                    "HANG_OPERATION": "connection",
                    "READY_FILE": ready_file
                }
            }
        }
    })
    .to_string();

    let result = common::expire_after_fixture_ready(
        CompressedServer::connect_mcp_config_json(common::max_config(None), &config),
        &ready_file,
        "connection",
        Duration::from_millis(500),
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "slow", "connection");
}

#[tokio::test]
async fn remote_http_connection_timeout_closes_in_flight_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut remote = JoinSet::new();
    remote.spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).await.unwrap();
        request
    });
    let backend = BackendServerConfig::new(
        "remote",
        format!("http://{address}/mcp"),
        Vec::<String>::new(),
    )
    .with_auth_mode(BackendAuthMode::ExplicitHeaders)
    .with_timeout(Duration::from_millis(500));

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        CompressedServer::connect_stdio(common::max_config(Some("remote")), backend),
    )
    .await
    .expect("configured timeout did not bound remote connection");
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "remote", "connection");

    let request = tokio::time::timeout(Duration::from_secs(2), remote.join_next())
        .await
        .expect("timed-out HTTP request remained connected")
        .unwrap()
        .unwrap();
    assert!(!request.is_empty());
}

#[tokio::test]
async fn remote_http_tool_timeout_is_hard_and_releases_in_flight_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = HangingHttpState {
        call_started: Arc::new(AtomicBool::new(false)),
        call_dropped: Arc::new(AtomicBool::new(false)),
        call_cancelled: Arc::new(AtomicBool::new(false)),
    };
    let app = Router::new()
        .route("/mcp", post(hanging_http_mcp))
        .with_state(state.clone());
    let mut remote = JoinSet::new();
    remote.spawn(async move { axum::serve(listener, app).await.unwrap() });
    let backend = BackendServerConfig::new(
        "remote",
        format!("http://{address}/mcp"),
        Vec::<String>::new(),
    )
    .with_auth_mode(BackendAuthMode::ExplicitHeaders)
    .with_timeout(Duration::from_millis(500));
    let server = CompressedServer::connect_stdio(common::max_config(Some("remote")), backend)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        server.invoke_tool("remote_invoke_tool", "hang", json!({})),
    )
    .await
    .expect("configured timeout did not hard-bound remote tool request");
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "remote", "call tool `hang`");
    assert!(state.call_started.load(Ordering::SeqCst));

    tokio::time::timeout(Duration::from_secs(2), async {
        while !state.call_dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed-out HTTP tool request remained active");
    drop(server);
    remote.shutdown().await;
}

#[tokio::test]
async fn remote_http_tool_timeout_cancels_the_backend_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = HangingHttpState {
        call_started: Arc::new(AtomicBool::new(false)),
        call_dropped: Arc::new(AtomicBool::new(false)),
        call_cancelled: Arc::new(AtomicBool::new(false)),
    };
    let app = Router::new()
        .route("/mcp", post(hanging_http_mcp))
        .with_state(state.clone());
    let mut remote = JoinSet::new();
    remote.spawn(async move { axum::serve(listener, app).await.unwrap() });
    let backend = BackendServerConfig::new(
        "remote",
        format!("http://{address}/mcp"),
        Vec::<String>::new(),
    )
    .with_auth_mode(BackendAuthMode::ExplicitHeaders)
    .with_timeout(Duration::from_millis(500));
    let server = CompressedServer::connect_stdio(common::max_config(Some("remote")), backend)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        server.invoke_tool("remote_invoke_tool", "hang", json!({})),
    )
    .await
    .expect("configured timeout did not hard-bound remote tool request");
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "remote", "call tool `hang`");
    assert!(state.call_started.load(Ordering::SeqCst));

    tokio::time::timeout(Duration::from_secs(3), async {
        while !state.call_cancelled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed-out HTTP tool request never sent notifications/cancelled");
    drop(server);
    remote.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn configured_timeout_bounds_tool_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let ready_file = temp.path().join("ready");
    let timeout = Duration::from_millis(500);
    let backend = common::backend("hanging", "hanging_server.py")
        .with_env([
            ("HANG_OPERATION", "discovery"),
            ("READY_FILE", ready_file.to_str().unwrap()),
        ])
        .with_timeout(timeout);

    let result = common::expire_after_fixture_ready(
        CompressedServer::connect_stdio(common::max_config(Some("hanging")), backend),
        &ready_file,
        "discovery",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "list tools");
}

#[tokio::test(start_paused = true)]
async fn configured_timeout_bounds_resource_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let ready_file = temp.path().join("ready");
    let timeout = Duration::from_millis(500);
    let backend = common::backend("hanging", "hanging_server.py")
        .with_env([
            ("HANG_OPERATION", "resource-discovery"),
            ("READY_FILE", ready_file.to_str().unwrap()),
        ])
        .with_timeout(timeout);

    let result = common::expire_after_fixture_ready(
        CompressedServer::connect_stdio(common::max_config(Some("hanging")), backend),
        &ready_file,
        "resource-discovery",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "list resources");
}

#[tokio::test(start_paused = true)]
async fn configured_timeout_bounds_prompt_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let ready_file = temp.path().join("ready");
    let timeout = Duration::from_millis(500);
    let backend = common::backend("hanging", "hanging_server.py")
        .with_env([
            ("HANG_OPERATION", "prompt-discovery"),
            ("READY_FILE", ready_file.to_str().unwrap()),
        ])
        .with_timeout(timeout);

    let result = common::expire_after_fixture_ready(
        CompressedServer::connect_stdio(common::max_config(Some("hanging")), backend),
        &ready_file,
        "prompt-discovery",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "list prompts");
}

#[tokio::test(start_paused = true)]
async fn configured_timeout_bounds_backend_tool_requests() {
    let temp = tempfile::tempdir().unwrap();
    let ready_file = temp.path().join("ready");
    let timeout = Duration::from_millis(500);
    let backend = common::backend("hanging", "hanging_server.py")
        .with_env([("READY_FILE", ready_file.to_str().unwrap())])
        .with_timeout(timeout);
    let server = common::drive_with_frozen_time(CompressedServer::connect_stdio(
        common::max_config(Some("hanging")),
        backend,
    ))
    .await
    .unwrap();

    assert_eq!(
        common::drive_with_frozen_time(server.invoke_tool(
            "hanging_invoke_tool",
            "fast",
            json!({})
        ))
        .await
        .unwrap(),
        "fast"
    );
    let result = common::expire_after_fixture_ready(
        server.invoke_tool("hanging_invoke_tool", "hang", json!({})),
        &ready_file,
        "tools/call",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "call tool `hang`");

    let result = common::expire_after_fixture_ready(
        server.read_resource("fixture://hanging-resource"),
        &ready_file,
        "resources/read",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(
        &error,
        "hanging",
        "read resource `fixture://hanging-resource`",
    );

    let result = common::expire_after_fixture_ready(
        server.get_prompt("hanging_prompt", None),
        &ready_file,
        "prompts/get",
        timeout,
    )
    .await;
    let error = result.unwrap_err();
    assert_backend_timeout(&error, "hanging", "get prompt `hanging_prompt`");
}
