use std::process::Stdio;

use rmcp::model::Prompt;
use rmcp::service::RunningService;
use rmcp::transport::auth::{AuthClient, AuthorizationManager};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

use crate::compression::engine::Tool;
use crate::oauth::{
    oauth_store_dir, open_authorization_url, remember_oauth_store, BrowserOpenStatus,
    FileCredentialStore, FileStateStore, OAuthCallbackListener,
};
use crate::server::backend::{backend_http_headers, BackendServerConfig, BackendTransport};
use crate::server::dynamic_http_client::DynamicAuthHttpClient;
use crate::Error;

#[derive(Debug)]
pub(crate) struct ConnectedBackend {
    pub public_name: String,
    pub client: RunningService<RoleClient, ()>,
    pub tools: Vec<Tool>,
    pub resources: Vec<String>,
    pub prompts: Vec<Prompt>,
}

pub(crate) async fn connect_backend(
    backend: BackendServerConfig,
    public_name: String,
    include_tools: &[String],
    exclude_tools: &[String],
) -> Result<ConnectedBackend, Error> {
    let client = match backend.transport {
        BackendTransport::Stdio => connect_stdio_backend(&backend).await?,
        BackendTransport::StreamableHttp => connect_streamable_http_backend(&backend).await?,
    };

    let rmcp_tools = client
        .list_all_tools()
        .await
        .map_err(|error| Error::Config(error.to_string()))?;
    let mut tools = rmcp_tools.into_iter().map(convert_tool).collect::<Vec<_>>();
    if !include_tools.is_empty() {
        tools.retain(|tool| include_tools.iter().any(|include| include == &tool.name));
    }
    if !exclude_tools.is_empty() {
        tools.retain(|tool| !exclude_tools.iter().any(|exclude| exclude == &tool.name));
    }

    let resources = client
        .list_all_resources()
        .await
        .map(|resources| {
            resources
                .into_iter()
                .map(|resource| resource.raw.uri)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let prompts = client.list_all_prompts().await.unwrap_or_default();

    Ok(ConnectedBackend {
        public_name,
        client,
        tools,
        resources,
        prompts,
    })
}

async fn connect_stdio_backend(
    backend: &BackendServerConfig,
) -> Result<RunningService<RoleClient, ()>, Error> {
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

    let transport = TokioChildProcess::new(command.configure(|_| {})).map_err(Error::Io)?;
    ().serve(transport)
        .await
        .map_err(|error| Error::Config(error.to_string()))
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
    let mut config = StreamableHttpClientTransportConfig::with_uri(backend.command.clone());
    let headers = backend_http_headers(backend)?;
    if backend.header_provider.is_none() && !headers.is_empty() {
        config = config.custom_headers(headers.clone());
    }
    if let Some(provider) = backend.header_provider.clone() {
        let client = DynamicAuthHttpClient::new(reqwest::Client::new(), headers, provider);
        let transport = StreamableHttpClientTransport::with_client(client, config);
        ().serve(transport)
            .await
            .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
    } else {
        let transport = StreamableHttpClientTransport::from_config(config);
        ().serve(transport)
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
                &[],
                &redirect_uri,
                Some(
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
        let callback = listener.wait_for_callback().map_err(Error::Io)?;
        state
            .handle_callback(&callback.code, &callback.state)
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
        client,
        StreamableHttpClientTransportConfig::with_uri(backend.command.clone()),
    );
    ().serve(transport)
        .await
        .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
}

fn oauth_http_client(backend: &BackendServerConfig) -> Result<reqwest::Client, Error> {
    let headers = backend_http_headers(backend)?.into_iter().collect();
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|error| Error::Config(format!("failed to build OAuth HTTP client: {error}")))
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

        assert!(server
            .join()
            .unwrap()
            .to_ascii_lowercase()
            .contains("x-tenant: tenant-123"));
    }

    #[tokio::test]
    async fn oauth_backend_validates_headers_before_authorization() {
        let backend = BackendServerConfig::new(
            "remote",
            "http://127.0.0.1:0/mcp",
            [] as [&str; 0],
        )
        .with_headers([("invalid header", "value")]);

        let error = connect_oauth_streamable_http_backend(&backend)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("invalid HTTP header name"));
    }
}
