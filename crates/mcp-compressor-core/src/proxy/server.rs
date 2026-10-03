//! Generic HTTP tool proxy server.
//!
//! Binds to `127.0.0.1:<port>` (random free port by default), generates a
//! `SessionToken` at startup, and routes `/health` and `/exec` requests.

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::Error;
use crate::proxy::auth::SessionToken;
use crate::server::compressed::CompressedServer;

#[derive(Debug)]
pub struct ToolProxyServer;

pub type BeforeExecHook =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> + Send + Sync>;

#[derive(Debug)]
pub struct RunningToolProxy {
    bridge_url: String,
    token: SessionToken,
    task: tokio::task::JoinHandle<()>,
    server: Arc<CompressedServer>,
}

#[derive(Clone)]
struct ProxyState {
    server: Arc<CompressedServer>,
    token: SessionToken,
    before_exec: Option<Arc<BeforeExec>>,
}

struct BeforeExec {
    hook: BeforeExecHook,
    request_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Deserialize)]
struct ExecRequest {
    tool: String,
    #[serde(default)]
    input: Value,
}

#[derive(Debug, Deserialize)]
struct WrapperInvokeInput {
    tool_name: String,
    #[serde(default)]
    tool_input: Value,
}

impl ToolProxyServer {
    pub async fn start(server: CompressedServer) -> Result<RunningToolProxy, Error> {
        Self::start_inner(server, None).await
    }

    pub async fn start_with_before_exec(
        server: CompressedServer,
        before_exec: BeforeExecHook,
    ) -> Result<RunningToolProxy, Error> {
        Self::start_inner(
            server,
            Some(Arc::new(BeforeExec {
                hook: before_exec,
                request_lock: Arc::new(Mutex::new(())),
            })),
        )
        .await
    }

    pub(crate) async fn start_with_before_exec_lock(
        server: CompressedServer,
        before_exec: BeforeExecHook,
        request_lock: Arc<Mutex<()>>,
    ) -> Result<RunningToolProxy, Error> {
        Self::start_inner(
            server,
            Some(Arc::new(BeforeExec {
                hook: before_exec,
                request_lock,
            })),
        )
        .await
    }

    async fn start_inner(
        server: CompressedServer,
        before_exec: Option<Arc<BeforeExec>>,
    ) -> Result<RunningToolProxy, Error> {
        let token = SessionToken::generate();
        let server = Arc::new(server);
        let state = ProxyState {
            server: Arc::clone(&server),
            token: token.clone(),
            before_exec,
        };

        let app = Router::new()
            .route("/health", get(health))
            .route("/exec", post(exec))
            .with_state(state);

        // std creates non-inheritable sockets on Windows, so later backend
        // subprocesses cannot keep this listener alive after the proxy stops.
        let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        let addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                eprintln!("mcp-compressor proxy server error: {error}");
            }
        });

        Ok(RunningToolProxy {
            bridge_url: format!("http://{addr}"),
            token,
            task,
            server,
        })
    }

    /// Wrap a connected compressed server for in-process (bridge-less) use.
    ///
    /// No HTTP listener, token, or background task is created. Callers dispatch
    /// tools directly via [`dispatch_exec`] using the returned shared handle.
    /// This is the preferred path for in-process SDK consumers that do not need
    /// to expose the session to out-of-process clients.
    pub fn in_process(server: CompressedServer) -> Arc<CompressedServer> {
        Arc::new(server)
    }
}

async fn health() -> Response {
    close_response(StatusCode::OK, "ok")
}

async fn exec(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    Json(request): Json<ExecRequest>,
) -> Response {
    if !authorized(&state.token, &headers) {
        return close_response(StatusCode::UNAUTHORIZED, "unauthorized");
    }

    let result = if let Some(before_exec) = &state.before_exec {
        {
            // The lock only serializes the auth refresh itself. Holding it
            // across dispatch would serialize every bridge request, so
            // concurrent generated-client calls would queue behind each other.
            let _refresh_guard = before_exec.request_lock.lock().await;
            if (before_exec.hook)().await.is_err() {
                return close_response(StatusCode::BAD_REQUEST, "auth provider refresh failed");
            }
        }
        dispatch_exec(&state.server, request.tool, request.input).await
    } else {
        dispatch_exec(&state.server, request.tool, request.input).await
    };

    match result {
        Ok(result) => close_response(StatusCode::OK, result),
        Err(error) => close_response(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

fn close_response(status: StatusCode, body: impl Into<String>) -> Response {
    let mut response = (status, body.into()).into_response();
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("close"),
    );
    response
}

/// Execute a frontend wrapper tool (or single-backend pass-through tool) against
/// a compressed server.
///
/// This is the shared dispatch used by both the HTTP `/exec` bridge endpoint and
/// the in-process SDK session path, so both transports produce identical results.
pub async fn dispatch_exec(
    server: &CompressedServer,
    tool: String,
    input: Value,
) -> Result<String, Error> {
    if tool.ends_with("_invoke_tool") || tool == "invoke_tool" {
        let wrapper_input: WrapperInvokeInput = serde_json::from_value(input)?;
        server
            .invoke_tool(&tool, &wrapper_input.tool_name, wrapper_input.tool_input)
            .await
    } else {
        server.invoke_single_backend_tool(&tool, input).await
    }
}

fn authorized(token: &SessionToken, headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|header| token.verify(header))
}

impl Drop for RunningToolProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RunningToolProxy {
    /// Stop the HTTP listener and wait for its task to release the bound socket.
    pub async fn shutdown(mut self) -> Result<(), tokio::task::JoinError> {
        self.task.abort();
        match (&mut self.task).await {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Shared handle to the underlying compressed server, used for in-process
    /// (bridge-less) dispatch by SDK sessions.
    pub fn server(&self) -> &Arc<CompressedServer> {
        &self.server
    }

    pub fn bridge_url(&self) -> &str {
        &self.bridge_url
    }

    pub fn token(&self) -> &SessionToken {
        &self.token
    }

    pub fn token_value(&self) -> &str {
        self.token.value()
    }

    pub fn health_url(&self) -> String {
        format!("{}/health", self.bridge_url)
    }

    pub fn exec_url(&self) -> String {
        format!("{}/exec", self.bridge_url)
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use crate::compression::CompressionLevel;
    use crate::server::{BackendServerConfig, CompressedServerConfig};
    use std::time::{Duration, Instant};

    async fn alpha_server() -> CompressedServer {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        CompressedServer::connect_multi_stdio(
            CompressedServerConfig {
                level: CompressionLevel::Max,
                server_name: Some("alpha".to_string()),
                ..CompressedServerConfig::default()
            },
            vec![BackendServerConfig::new(
                "alpha",
                python,
                [fixture.to_string_lossy().into_owned()],
            )],
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn auth_refresh_does_not_serialize_concurrent_bridge_requests() {
        let server = alpha_server().await;
        let hook: BeforeExecHook = Arc::new(|| Box::pin(async { Ok(()) }));
        let proxy = ToolProxyServer::start_with_before_exec(server, hook)
            .await
            .unwrap();
        let url = format!("{}/exec", proxy.bridge_url());
        let token = proxy.token_value().to_string();
        let client = reqwest::Client::new();

        let delay = Duration::from_millis(700);
        let started = Instant::now();
        let mut calls = Vec::new();
        for index in 0..3 {
            let client = client.clone();
            let url = url.clone();
            let token = token.clone();
            calls.push(tokio::spawn(async move {
                client
                    .post(url)
                    .bearer_auth(token)
                    .json(&serde_json::json!({
                        "tool": "alpha_alpha_invoke_tool",
                        "input": {
                            "tool_name": "slow_echo",
                            "tool_input": { "message": index.to_string(), "seconds": 0.7 }
                        }
                    }))
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
            }));
        }
        for call in calls {
            let body = call.await.unwrap();
            assert!(body.contains("alpha:"), "unexpected body: {body}");
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed < delay * 2,
            "three concurrent bridge requests took {elapsed:?}; the auth refresh lock must not \
             be held across dispatch"
        );
    }
}
