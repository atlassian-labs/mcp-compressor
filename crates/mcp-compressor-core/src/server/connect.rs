use std::collections::HashMap;
use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::model::{ClientJsonRpcMessage, Prompt};
use rmcp::service::{ClientLifecycleMode, ClientServiceExt, RunningService};
use rmcp::transport::auth::{AuthClient, AuthorizationManager, AuthorizationRequest};
use rmcp::transport::streamable_http_client::{
    SseError, StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError,
    StreamableHttpPostResponse,
};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{Peer, RoleClient, ServiceExt};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use process_wrap::tokio::CommandWrap;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;

use crate::Error;
use crate::compression::engine::Tool;
use crate::oauth::{
    BrowserOpenStatus, FileCredentialStore, FileStateStore, OAuthCallbackListener, oauth_store_dir,
    open_authorization_url, remember_oauth_store,
};
use crate::server::backend::{BackendServerConfig, BackendTransport, backend_http_headers};
use crate::server::dynamic_http_client::DynamicAuthHttpClient;

#[derive(Debug)]
pub(crate) struct ConnectedBackend {
    pub public_name: String,
    pub backend_name: String,
    pub client: Peer<RoleClient>,
    pub tools: Vec<Tool>,
    pub resources: Vec<String>,
    pub prompts: Vec<Prompt>,
    pub timeout: Option<Duration>,
    process_id: Option<u32>,
    transport: BackendTransport,
    service: tokio::sync::Mutex<RunningService<RoleClient, ()>>,
}

impl ConnectedBackend {
    #[cfg(test)]
    pub(crate) fn for_test(
        name: &str,
        service: RunningService<RoleClient, ()>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            public_name: name.to_string(),
            backend_name: name.to_string(),
            client: service.peer().clone(),
            tools: Vec::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
            timeout,
            process_id: None,
            transport: BackendTransport::StreamableHttp,
            service: tokio::sync::Mutex::new(service),
        }
    }

    /// Release the backend without consuming it.
    ///
    /// Session shutdown must not depend on being the last owner of the
    /// server: while an HTTP bridge is draining, its connection tasks still
    /// hold a clone, and an ownership-based shutdown would silently skip the
    /// release and leak the backend process tree.
    pub async fn shutdown_shared(&self) -> Result<(), Error> {
        let mut service = self.service.lock().await;
        if !service.is_closed() && matches!(self.transport, BackendTransport::Stdio) {
            terminate_owned_process_tree(self.process_id).await;
        }
        service
            .close()
            .await
            .map(|_| ())
            .map_err(|error| Error::Io(std::io::Error::other(error)))
    }
}

pub(crate) async fn connect_backend(
    backend: BackendServerConfig,
    public_name: String,
    include_tools: &[String],
    exclude_tools: &[String],
) -> Result<ConnectedBackend, Error> {
    if backend.timeout == Some(Duration::ZERO) {
        return Err(Error::Validation(format!(
            "backend {:?} timeout must be greater than zero",
            backend.name
        )));
    }
    let timeout = backend.timeout;
    let backend_name = backend.name.clone();
    let (client, process_id) = match backend.transport {
        BackendTransport::Stdio => connect_stdio_backend(&backend, &backend_name).await?,
        BackendTransport::StreamableHttp => {
            // An interactive OAuth login waits for a human in a browser, so the
            // backend timeout must not apply to it. Enforcing it there would
            // report `BackendTimeout` for a login that actually succeeds.
            let client = if backend.should_use_oauth() {
                connect_streamable_http_backend(&backend).await?
            } else {
                backend_operation(
                    timeout,
                    &backend_name,
                    "connection",
                    connect_streamable_http_backend(&backend),
                )
                .await?
            };
            (client, None)
        }
    };

    let rmcp_tools = match backend_operation(timeout, &backend_name, "list tools", async {
        client.list_all_tools().await.map_err(service_error)
    })
    .await
    {
        Ok(tools) => tools,
        Err(error) => {
            cleanup_after_discovery_failure(&client, process_id).await;
            return Err(error.into());
        }
    };
    let mut tools = rmcp_tools.into_iter().map(convert_tool).collect::<Vec<_>>();
    if !include_tools.is_empty() {
        tools.retain(|tool| include_tools.iter().any(|include| include == &tool.name));
    }
    if !exclude_tools.is_empty() {
        tools.retain(|tool| !exclude_tools.iter().any(|exclude| exclude == &tool.name));
    }

    let resources = match backend_operation(timeout, &backend_name, "list resources", async {
        client.list_all_resources().await.map_err(service_error)
    })
    .await
    {
        Ok(resources) => resources
            .into_iter()
            .map(|resource| resource.uri)
            .collect::<Vec<_>>(),
        Err(error) if optional_discovery_error_is_ignorable(&error) => Vec::new(),
        Err(error) => {
            cleanup_after_discovery_failure(&client, process_id).await;
            return Err(error.into());
        }
    };
    let prompts = match backend_operation(timeout, &backend_name, "list prompts", async {
        client.list_all_prompts().await.map_err(service_error)
    })
    .await
    {
        Ok(prompts) => prompts,
        Err(error) if optional_discovery_error_is_ignorable(&error) => Vec::new(),
        Err(error) => {
            cleanup_after_discovery_failure(&client, process_id).await;
            return Err(error.into());
        }
    };

    Ok(ConnectedBackend {
        public_name,
        backend_name,
        client: client.peer().clone(),
        tools,
        resources,
        prompts,
        timeout,
        process_id,
        transport: backend.transport,
        service: tokio::sync::Mutex::new(client),
    })
}

async fn connect_stdio_backend(
    backend: &BackendServerConfig,
    backend_name: &str,
) -> Result<(RunningService<RoleClient, ()>, Option<u32>), Error> {
    let mut command = tokio::process::Command::new(&backend.command);
    command
        .args(&backend.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    command.stderr(Stdio::inherit());
    if let Some(cwd) = &backend.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &backend.env {
        command.env(key, value);
    }

    let mut command = CommandWrap::from(command.configure(|_| {}));
    #[cfg(windows)]
    command.wrap(JobObject);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    let transport = TokioChildProcess::new(command).map_err(Error::Io)?;
    let process_id = transport.id();
    let cancellation = CancellationToken::new();
    let connect = ().serve_with_ct(transport, cancellation.clone());
    tokio::pin!(connect);
    match backend.timeout {
        Some(timeout) => tokio::select! {
            biased;
            result = &mut connect => result
                .map(|client| (client, process_id))
                .map_err(|error| Error::Config(error.to_string())),
            _ = tokio::time::sleep(timeout) => {
                terminate_owned_process_tree(process_id).await;
                cancellation.cancel();
                let _ = connect.await;
                Err(timeout_error(backend_name, "connection", timeout))
            }
        },
        None => connect
            .await
            .map(|client| (client, process_id))
            .map_err(|error| Error::Config(error.to_string())),
    }
}

pub(crate) async fn backend_operation<T, F>(
    timeout: Option<Duration>,
    backend: &str,
    operation: &str,
    future: F,
) -> Result<T, BackendOperationError>
where
    F: Future<Output = Result<T, Error>>,
{
    match timeout {
        Some(timeout) => {
            let result = tokio::time::timeout(timeout, future).await.map_err(|_| {
                BackendOperationError::Timeout {
                    backend: backend.to_string(),
                    operation: operation.to_string(),
                    timeout,
                }
            })?;
            result.map_err(BackendOperationError::Failure)
        }
        None => future.await.map_err(BackendOperationError::Failure),
    }
}

#[derive(Debug)]
pub(crate) enum BackendOperationError {
    Failure(Error),
    Timeout {
        backend: String,
        operation: String,
        timeout: Duration,
    },
}

impl From<BackendOperationError> for Error {
    fn from(error: BackendOperationError) -> Self {
        match error {
            BackendOperationError::Failure(error) => error,
            BackendOperationError::Timeout {
                backend,
                operation,
                timeout,
            } => Error::Config(format!(
                "backend {backend:?} timed out during {operation} after {timeout:?}"
            )),
        }
    }
}

pub(crate) fn timeout_error(backend: &str, operation: &str, timeout: Duration) -> Error {
    Error::Config(format!(
        "backend {backend:?} timed out during {operation} after {timeout:?}"
    ))
}

fn optional_discovery_error_is_ignorable(error: &BackendOperationError) -> bool {
    matches!(error, BackendOperationError::Failure(Error::Config(_)))
}

#[cfg(test)]
mod optional_discovery_tests {
    use super::*;

    #[test]
    fn optional_discovery_ignores_remote_config_errors_but_not_timeouts() {
        let remote_error = BackendOperationError::Failure(Error::Config(
            "backend timed out during cache refresh".to_string(),
        ));

        assert!(optional_discovery_error_is_ignorable(&remote_error));
        assert!(!optional_discovery_error_is_ignorable(
            &BackendOperationError::Timeout {
                backend: "backend".to_string(),
                operation: "list resources".to_string(),
                timeout: Duration::from_millis(500),
            },
        ));
    }
}

fn service_error(error: rmcp::service::ServiceError) -> Error {
    Error::Config(error.to_string())
}

async fn cleanup_after_discovery_failure(
    client: &RunningService<RoleClient, ()>,
    process_id: Option<u32>,
) {
    terminate_owned_process_tree(process_id).await;
    client.cancellation_token().cancel();
}

#[cfg(windows)]
async fn terminate_owned_process_tree(process_id: Option<u32>) {
    let Some(process_id) = process_id else {
        return;
    };
    match tokio::process::Command::new("taskkill")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("failed to terminate backend process tree {process_id}: {status}"),
        Err(error) => eprintln!("failed to terminate backend process tree {process_id}: {error}"),
    }
}

#[cfg(unix)]
async fn terminate_owned_process_tree(process_id: Option<u32>) {
    let Some(process_id) = process_id else {
        return;
    };
    let result = unsafe { libc::kill(-(process_id as i32), libc::SIGKILL) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            eprintln!("failed to terminate backend process group {process_id}: {error}");
        }
    }
}

async fn connect_streamable_http_backend(
    backend: &BackendServerConfig,
) -> Result<RunningService<RoleClient, ()>, Error> {
    if !backend.args.is_empty() {
        return Err(Error::Config(
            "streamable HTTP backend URLs do not accept command arguments".to_string(),
        ));
    }
    if backend.should_use_oauth() {
        return connect_oauth_streamable_http_backend(backend).await;
    }
    let http_client = backend_http_client(backend)?;
    let mut config = StreamableHttpClientTransportConfig::with_uri(backend.command.clone());
    let headers = backend_http_headers(backend)?;
    if backend.header_provider.is_none() && !headers.is_empty() {
        config = config.custom_headers(headers.clone());
    }
    if let Some(provider) = backend.header_provider.clone() {
        let client = DynamicAuthHttpClient::new(http_client, headers, provider);
        let transport = StreamableHttpClientTransport::with_client(
            BackendHttpClient::new(client, backend.timeout),
            config,
        );
        ().serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
            .await
            .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
    } else {
        let transport = StreamableHttpClientTransport::with_client(
            BackendHttpClient::new(http_client, backend.timeout),
            config,
        );
        ().serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
            .await
            .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
    }
}

async fn connect_oauth_streamable_http_backend(
    backend: &BackendServerConfig,
) -> Result<RunningService<RoleClient, ()>, Error> {
    let http_client = oauth_http_client(backend)?;
    let mut manager = AuthorizationManager::new(backend.command.as_str())
        .await
        .map_err(|error| Error::Config(format!("failed to initialize OAuth manager: {error}")))?;
    let store_dir = oauth_store_dir(&backend.command, &backend.name);
    remember_oauth_store(&backend.command, &backend.name, &store_dir).map_err(Error::Io)?;
    let credential_store = FileCredentialStore::new(store_dir.join("credentials.json"));
    let state_store = FileStateStore::new(store_dir.join("state"));
    manager.set_credential_store(credential_store.clone());
    manager.set_state_store(state_store.clone());

    if !manager
        .initialize_from_store()
        .await
        .map_err(|error| Error::Config(format!("failed to load OAuth credentials: {error}")))?
    {
        let listener = OAuthCallbackListener::bind().map_err(Error::Io)?;
        let redirect_uri = listener.redirect_uri().to_string();
        let mut state = rmcp::transport::auth::OAuthState::new(backend.command.as_str(), None)
            .await
            .map_err(|error| Error::Config(format!("failed to initialize OAuth state: {error}")))?;
        if let rmcp::transport::auth::OAuthState::Unauthorized(ref mut state_manager) = state {
            state_manager.set_credential_store(credential_store);
            state_manager.set_state_store(state_store);
        }
        state
            .start_authorization(
                AuthorizationRequest::new(redirect_uri).with_client_name(
                    backend
                        .oauth_app_name
                        .as_deref()
                        .unwrap_or("mcp-compressor"),
                ),
            )
            .await
            .map_err(|error| {
                Error::Config(format!("failed to start OAuth authorization: {error}"))
            })?;
        let auth_url = state.get_authorization_url().await.map_err(|error| {
            Error::Config(format!("failed to get OAuth authorization URL: {error}"))
        })?;
        match open_authorization_url(&auth_url) {
            Ok(BrowserOpenStatus::Opened) => {
                eprintln!("Opened browser to authorize {name}.", name = backend.name);
            }
            Ok(BrowserOpenStatus::Disabled) => {
                eprintln!("Browser opening disabled for {name}.", name = backend.name);
            }
            Err(error) => {
                eprintln!(
                    "Failed to open browser for {name}: {error}",
                    name = backend.name
                );
            }
        }
        eprintln!(
            "If the browser did not open, authorize {name} with this URL:\n{auth_url}",
            name = backend.name
        );
        let callback = tokio::task::spawn_blocking(move || listener.wait_for_callback())
            .await
            .map_err(|error| Error::Config(format!("OAuth callback task failed: {error}")))?
            .map_err(Error::Io)?;
        state
            .handle_callback_with_issuer(
                &callback.code,
                &callback.state,
                callback.iss.as_deref(),
            )
            .await
            .map_err(|error| {
                Error::Config(format!("failed to complete OAuth authorization: {error}"))
            })?;
        manager = state.into_authorization_manager().ok_or_else(|| {
            Error::Config("OAuth authorization did not produce an authorized manager".to_string())
        })?;
    }

    let client = AuthClient::new(http_client, manager);
    let transport = StreamableHttpClientTransport::with_client(
        BackendHttpClient::new(client, backend.timeout),
        StreamableHttpClientTransportConfig::with_uri(backend.command.clone()),
    );
    ().serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
        .await
        .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
}

fn oauth_http_client(backend: &BackendServerConfig) -> Result<reqwest::Client, Error> {
    let headers = backend_http_headers(backend)?.into_iter().collect();
    backend_http_client_builder(backend)
        .default_headers(headers)
        .build()
        .map_err(|error| Error::Config(format!("failed to build OAuth HTTP client: {error}")))
}

fn backend_http_client(backend: &BackendServerConfig) -> Result<reqwest::Client, Error> {
    backend_http_client_builder(backend)
        .build()
        .map_err(Error::Http)
}

fn backend_http_client_builder(backend: &BackendServerConfig) -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder();
    if let Some(timeout) = backend.timeout {
        builder = builder.connect_timeout(timeout);
    }
    builder
}

#[derive(Clone)]
struct BackendHttpClient<C> {
    inner: C,
    timeout: Option<Duration>,
}

impl<C> BackendHttpClient<C> {
    fn new(inner: C, timeout: Option<Duration>) -> Self {
        Self { inner, timeout }
    }

    async fn request<T, E>(
        &self,
        future: impl Future<Output = Result<T, StreamableHttpError<E>>>,
    ) -> Result<T, StreamableHttpError<E>>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        match self.timeout {
            Some(timeout) => {
                // Leave the existing 100ms grace for rmcp's queued cancellation.
                tokio::time::timeout(timeout.saturating_add(Duration::from_millis(100)), future)
                    .await
                    .map_err(|_| {
                        StreamableHttpError::Io(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "backend HTTP request timed out",
                        ))
                    })?
            }
            None => future.await,
        }
    }
}

impl<C: StreamableHttpClient + Sync> StreamableHttpClient for BackendHttpClient<C> {
    type Error = C::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.request(
            self.inner
                .post_message(uri, message, session_id, auth_header, custom_headers),
        )
        .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<
        BoxStream<'static, Result<sse_stream::Sse, SseError>>,
        StreamableHttpError<Self::Error>,
    > {
        // Bound stream establishment, not the lifetime of the returned SSE body.
        self.request(self.inner.get_stream(
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
        ))
        .await
    }

    // rmcp's transport calls the size-limited variants; forward them so the
    // inner client keeps enforcing the transport's SSE event size limit.
    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.request(self.inner.post_message_with_max_sse_event_size(
            uri,
            message,
            session_id,
            auth_header,
            custom_headers,
            max_sse_event_size,
        ))
        .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<
        BoxStream<'static, Result<sse_stream::Sse, SseError>>,
        StreamableHttpError<Self::Error>,
    > {
        // Bound stream establishment, not the lifetime of the returned SSE body.
        self.request(self.inner.get_stream_with_max_sse_event_size(
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
            max_sse_event_size,
        ))
        .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        self.request(
            self.inner
                .delete_session(uri, session_id, auth_header, custom_headers),
        )
        .await
    }
}

fn remote_backend_error(uri: &str, error: String) -> Error {
    let auth_hint = if error.contains("401")
        || error.contains("403")
        || error.contains("WWW-Authenticate")
        || error.to_ascii_lowercase().contains("unauthorized")
    {
        "\n\nThis remote MCP server appears to require authentication. \
For direct URL mode, pass explicit backend headers with the full command, for example: \
`mcp-compressor -- <url> -H \"Authorization=Bearer <token>\"`. Use native OAuth by omitting the `Authorization` header; other configured headers are sent alongside OAuth. For MCP JSON config, set valid headers in the server headers object."
    } else {
        "\n\nIf this remote MCP server requires authentication, use native OAuth or configure explicit headers. For direct URL mode, pass explicit backend headers with the full command, \
for example: `mcp-compressor -- <url> -H \"Authorization=Bearer <token>\"`. Use native OAuth by omitting the `Authorization` header; other configured headers are sent alongside OAuth. For MCP JSON config, set valid headers in the server headers object."
    };
    Error::Config(format!(
        "failed to initialize remote streamable HTTP backend {uri}: {error}{auth_hint}"
    ))
}

fn convert_tool(tool: rmcp::model::Tool) -> Tool {
    Tool::new(
        tool.name.to_string(),
        tool.description.map(|description| description.to_string()),
        Value::Object((*tool.input_schema).clone()),
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shutdown_reaps_backend_before_returning() {
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lifecycle_server.py");
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("backend.pid");
        let backend = connect_backend(
            BackendServerConfig::new(
                "lifecycle",
                python,
                [
                    fixture.to_string_lossy().into_owned(),
                    "--pid-file".to_string(),
                    pid_file.to_string_lossy().into_owned(),
                ],
            ),
            "lifecycle".to_string(),
            &[],
            &[],
        )
        .await
        .unwrap();
        let pid = backend.process_id.unwrap();
        let child_pid = std::fs::read_to_string(pid_file.with_extension("child.pid")).unwrap();

        backend.shutdown_shared().await.unwrap();
        let state_after_close = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        let child_state_after_close = std::fs::read_to_string(format!("/proc/{child_pid}/stat"));
        backend.shutdown_shared().await.unwrap();

        assert!(
            matches!(&state_after_close, Err(error) if error.kind() == std::io::ErrorKind::NotFound),
            "backend still existed immediately after shutdown: {state_after_close:?}"
        );
        // The grandchild is orphaned once the backend dies and is reaped by
        // whichever process inherits it (PID 1 in CI containers), so a zombie
        // counts as terminated; only the direct backend is ours to reap.
        let child_terminated = match &child_state_after_close {
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            Ok(stat) => stat
                .rsplit_once(") ")
                .is_some_and(|(_, fields)| fields.starts_with('Z')),
        };
        assert!(
            child_terminated,
            "backend child still running immediately after shutdown: {child_state_after_close:?}"
        );
    }

    #[tokio::test]
    async fn oauth_http_client_sends_configured_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        let backend =
            BackendServerConfig::new("remote", format!("http://{address}/mcp"), [] as [&str; 0])
                .with_headers([("X-Tenant", "tenant-123")]);

        oauth_http_client(&backend)
            .unwrap()
            .get(format!("http://{address}/mcp"))
            .send()
            .await
            .unwrap();

        assert!(
            server
                .join()
                .unwrap()
                .to_ascii_lowercase()
                .contains("x-tenant: tenant-123")
        );
    }

    #[tokio::test]
    async fn oauth_backend_validates_headers_before_authorization() {
        let backend = BackendServerConfig::new("remote", "http://127.0.0.1:0/mcp", [] as [&str; 0])
            .with_headers([("invalid header", "value")]);

        let error = connect_oauth_streamable_http_backend(&backend)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("invalid HTTP header name"));
    }
}

#[cfg(test)]
mod http_timeout_tests {
    use super::*;
    use futures::StreamExt;
    use rmcp::transport::streamable_http_client::StreamableHttpClient;
    use std::convert::Infallible;
    use tokio::task::JoinSet;

    /// Serve a response that is delivered in several small chunks. Each gap is
    /// short, but the total transfer is far longer than the backend timeout —
    /// exactly the shape of a healthy long-lived SSE stream.
    async fn start_dripping_server(tasks: &mut JoinSet<()>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/stream",
            axum::routing::get(|| async {
                let chunks = futures::stream::unfold(0_u8, |index| async move {
                    if index >= 10 {
                        return None;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Some((
                        Ok::<_, Infallible>(axum::body::Bytes::from_static(b"tick")),
                        index + 1,
                    ))
                });
                axum::body::Body::from_stream(chunks)
            }),
        );
        tasks.spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/stream")
    }

    fn http_backend_with_timeout(url: &str, timeout: Duration) -> BackendServerConfig {
        BackendServerConfig::new("remote", url, Vec::<String>::new()).with_timeout(timeout)
    }

    /// A configured backend timeout must bound a stalled backend without
    /// severing a slow-but-healthy stream. A total request deadline would abort
    /// this transfer and cause permanent reconnect churn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_timeout_does_not_sever_long_lived_streams() {
        let mut tasks = JoinSet::new();
        let url = start_dripping_server(&mut tasks).await;
        let backend = http_backend_with_timeout(&url, Duration::from_millis(400));

        let client = backend_http_client(&backend).unwrap();
        let body = client
            .get(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        assert_eq!(body.len(), 40);
        tasks.shutdown().await;
    }

    #[tokio::test]
    async fn backend_timeout_preserves_idle_sse_until_later_data() {
        let mut tasks = JoinSet::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/stream",
            axum::routing::get(|| async {
                let events = futures::stream::once(async {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Ok::<_, Infallible>(
                        axum::response::sse::Event::default().event("message").data(
                            r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
                        ),
                    )
                });
                axum::response::Sse::new(events)
            }),
        );
        tasks.spawn(async move { axum::serve(listener, app).await.unwrap() });
        let url = format!("http://{address}/stream");
        let backend = http_backend_with_timeout(&url, Duration::from_millis(100));
        let client =
            BackendHttpClient::new(backend_http_client(&backend).unwrap(), backend.timeout);
        let mut stream = client
            .get_stream(url.into(), Some("session".into()), None, None, Default::default())
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(3), stream.next())
            .await
            .expect("later SSE event never arrived")
            .expect("idle SSE stream closed")
            .expect("idle SSE stream failed");
        let message: rmcp::model::ServerJsonRpcMessage =
            serde_json::from_str(event.data.as_deref().unwrap()).unwrap();
        assert!(matches!(
            message,
            rmcp::model::ServerJsonRpcMessage::Notification(_)
        ));
        drop(stream);
        tasks.shutdown().await;
    }

    /// A backend that never sends headers must still hit the HTTP deadline.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_timeout_still_aborts_a_stalled_backend() {
        let mut tasks = JoinSet::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tasks.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(stream);
        });
        let url = format!("http://{addr}/stalled");
        let backend = http_backend_with_timeout(&url, Duration::from_millis(200));

        let client =
            BackendHttpClient::new(backend_http_client(&backend).unwrap(), backend.timeout);
        let result = client
            .get_stream(url.into(), Some("session".into()), None, None, Default::default())
            .await;

        assert!(matches!(
            result,
            Err(StreamableHttpError::Io(error)) if error.kind() == std::io::ErrorKind::TimedOut
        ));
        tasks.shutdown().await;
    }
}
