//! Generic HTTP tool proxy server.
//!
//! Binds to `127.0.0.1:<port>` (random free port by default), generates a
//! `SessionToken` at startup, and routes `/health` and `/exec` requests.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;
use tokio::net::TcpListener;

use crate::proxy::auth::SessionToken;
use crate::server::compressed::CompressedServer;
use crate::Error;

#[derive(Debug)]
pub struct ToolProxyServer;

#[derive(Debug)]
pub struct RunningToolProxy {
    bridge_url: String,
    token: SessionToken,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct ProxyState {
    server: Arc<CompressedServer>,
    token: SessionToken,
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
        let token = SessionToken::generate();
        let state = ProxyState {
            server: Arc::new(server),
            token: token.clone(),
        };

        let app = Router::new()
            .route("/health", get(health))
            .route("/exec", post(exec))
            .with_state(state);

        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
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
        })
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

    match dispatch_exec(&state.server, request).await {
        Ok(result) => close_response(StatusCode::OK, result),
        Err(error) => close_response(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

fn close_response(status: StatusCode, body: impl Into<String>) -> Response {
    let mut response = (status, body.into()).into_response();
    response
        .headers_mut()
        .insert(header::CONNECTION, header::HeaderValue::from_static("close"));
    response
}

async fn dispatch_exec(server: &CompressedServer, request: ExecRequest) -> Result<String, Error> {
    if request.tool.ends_with("_invoke_tool") || request.tool == "invoke_tool" {
        let wrapper_input: WrapperInvokeInput = serde_json::from_value(request.input)?;
        server
            .invoke_tool(&request.tool, &wrapper_input.tool_name, wrapper_input.tool_input)
            .await
    } else {
        server
            .invoke_single_backend_tool(&request.tool, request.input)
            .await
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
