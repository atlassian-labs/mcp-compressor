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
    if !headers.is_empty() {
        config = config.custom_headers(headers);
    }
    let transport = StreamableHttpClientTransport::from_config(config);
    ().serve(transport)
        .await
        .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
}

async fn connect_oauth_streamable_http_backend(
    backend: &BackendServerConfig,
) -> Result<RunningService<RoleClient, ()>, Error> {
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
            .start_authorization(&[], &redirect_uri, Some("mcp-compressor"))
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

    let client = AuthClient::new(reqwest::Client::default(), manager);
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(backend.command.clone()),
    );
    ().serve(transport)
        .await
        .map_err(|error| remote_backend_error(&backend.command, error.to_string()))
}

fn remote_backend_error(uri: &str, error: String) -> Error {
    let auth_hint = if error.contains("401")
        || error.contains("403")
        || error.contains("WWW-Authenticate")
        || error.to_ascii_lowercase().contains("unauthorized")
    {
        "\n\nThis remote MCP server appears to require authentication. \
Pass explicit backend headers after the URL, for example: \
`-- <url> -H \"Authorization=Bearer <token>\"`. Native OAuth support is not implemented yet."
    } else {
        "\n\nIf this remote MCP server requires authentication, pass explicit backend headers after the URL, \
for example: `-- <url> -H \"Authorization=Bearer <token>\"`. Native OAuth support is not implemented yet."
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
