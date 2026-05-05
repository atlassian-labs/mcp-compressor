//! `CompressedServer` — the top-level object that owns the backend client,
//! tool cache, and compression engine, and exposes them via a frontend MCP server.
//!
//! This file intentionally exposes the runtime API that integration tests and
//! language bindings should target. Method bodies remain `todo!()` until the
//! Phase 1 runtime is implemented.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Stdio;
use std::str::FromStr;

use axum::http::{HeaderName, HeaderValue};
use rmcp::model::{
    CallToolRequestParams, Content, GetPromptRequestParams, GetPromptResult, Prompt, RawContent,
    ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::RunningService;
use rmcp::transport::auth::{AuthClient, AuthorizationManager};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compression::engine::{CompressionEngine, Tool};
use crate::compression::CompressionLevel;
use crate::config::topology::MCPConfig;
use crate::oauth::{
    oauth_store_dir, open_authorization_url, remember_oauth_store, BrowserOpenStatus,
    FileCredentialStore, FileStateStore, OAuthCallbackListener,
};
use crate::Error;

/// Transport type used to reach an upstream MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTransport {
    /// Spawn a local command and speak MCP over stdio.
    Stdio,
    /// Connect to a remote streamable HTTP MCP endpoint.
    StreamableHttp,
}

/// Authentication strategy for a remote upstream MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendAuthMode {
    /// Match Python parity: explicit `Authorization` headers are used as-is;
    /// otherwise native OAuth should be attempted for remote HTTP backends.
    Auto,
    /// Use explicit backend headers only; never start OAuth.
    ExplicitHeaders,
    /// Force native OAuth. This requires the OAuth runtime flow to be implemented.
    OAuth,
}

/// Configuration for one upstream MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub transport: BackendTransport,
    pub headers: HashMap<String, String>,
    pub auth_mode: BackendAuthMode,
}

impl BackendServerConfig {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let command = command.into();
        let transport = if is_http_url(&command) {
            BackendTransport::StreamableHttp
        } else {
            BackendTransport::Stdio
        };
        let raw_args = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let (args, headers, auth_mode) = if transport == BackendTransport::StreamableHttp {
            parse_http_backend_args(raw_args)
        } else {
            (raw_args, HashMap::new(), BackendAuthMode::Auto)
        };
        Self {
            name: name.into(),
            command,
            args,
            env: HashMap::new(),
            transport,
            headers,
            auth_mode,
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        self
    }
    pub fn with_headers(
        mut self,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.headers = headers
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        self
    }

    pub fn with_auth_mode(mut self, auth_mode: BackendAuthMode) -> Self {
        self.auth_mode = auth_mode;
        self
    }

    pub fn has_authorization_header(&self) -> bool {
        self.headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    }

    pub fn should_use_oauth(&self) -> bool {
        self.transport == BackendTransport::StreamableHttp
            && match self.auth_mode {
                BackendAuthMode::Auto => !self.has_authorization_header(),
                BackendAuthMode::ExplicitHeaders => false,
                BackendAuthMode::OAuth => true,
            }
    }
}

/// Frontend tool-surface mode exposed by the proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyTransformMode {
    /// Normal compressed MCP mode: get_tool_schema/invoke_tool/(list_tools at max).
    CompressedTools,
    /// CLI mode: expose one help tool per configured server and route generated clients through /exec.
    Cli,
    /// Just Bash mode: expose one bash tool plus per-server help tools.
    JustBash,
}

/// How upstream backend servers are supplied to the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendConfigSource {
    /// Direct command/argv input, e.g. `python alpha_server.py`.
    Command,
    /// JSON MCP config input with one `mcpServers` entry.
    SingleServerJsonConfig,
    /// JSON MCP config input with multiple `mcpServers` entries.
    MultiServerJsonConfig,
}

/// Compression/runtime options shared by single-server and multi-server modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedServerConfig {
    pub level: CompressionLevel,
    pub server_name: Option<String>,
    pub include_tools: Vec<String>,
    pub exclude_tools: Vec<String>,
    pub toonify: bool,
    pub transform_mode: ProxyTransformMode,
    pub config_source: BackendConfigSource,
}

impl Default for CompressedServerConfig {
    fn default() -> Self {
        Self {
            level: CompressionLevel::default(),
            server_name: None,
            include_tools: Vec::new(),
            exclude_tools: Vec::new(),
            toonify: false,
            transform_mode: ProxyTransformMode::CompressedTools,
            config_source: BackendConfigSource::Command,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JustBashProviderSpec {
    pub provider_name: String,
    pub help_tool_name: String,
    pub tools: Vec<JustBashCommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JustBashCommandSpec {
    pub command_name: String,
    pub backend_tool_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub invoke_tool_name: String,
}

/// Handle for a frontend MCP server running over streamable HTTP.
#[derive(Debug, Clone)]
pub struct RunningCompressedServer {
    addr: SocketAddr,
}

impl RunningCompressedServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Connected compressor runtime.
#[derive(Debug)]
pub struct CompressedServer {
    config: CompressedServerConfig,
    backends: Vec<ConnectedBackend>,
}

#[derive(Debug)]
struct ConnectedBackend {
    public_name: String,
    client: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
    resources: Vec<String>,
    prompts: Vec<Prompt>,
}

impl CompressedServer {
    /// Connect to one upstream stdio MCP server.
    pub async fn connect_stdio(
        config: CompressedServerConfig,
        backend: BackendServerConfig,
    ) -> Result<Self, Error> {
        let public_name = config
            .server_name
            .clone()
            .unwrap_or_else(|| backend.name.clone());
        let backend = connect_backend(
            backend,
            public_name,
            &config.include_tools,
            &config.exclude_tools,
        )
        .await?;
        Ok(Self {
            config,
            backends: vec![backend],
        })
    }

    /// Connect to multiple upstream stdio MCP servers.
    pub async fn connect_multi_stdio(
        config: CompressedServerConfig,
        backends: Vec<BackendServerConfig>,
    ) -> Result<Self, Error> {
        let suite_prefix = config.server_name.clone();
        let mut connected = Vec::with_capacity(backends.len());
        for backend in backends {
            let public_name = match &suite_prefix {
                Some(prefix) => format!("{prefix}_{}", backend.name),
                None => backend.name.clone(),
            };
            connected.push(
                connect_backend(
                    backend,
                    public_name,
                    &config.include_tools,
                    &config.exclude_tools,
                )
                .await?,
            );
        }
        Ok(Self {
            config,
            backends: connected,
        })
    }

    /// Connect using a JSON MCP config document containing one or more `mcpServers` entries.
    pub async fn connect_mcp_config_json(
        config: CompressedServerConfig,
        mcp_config_json: &str,
    ) -> Result<Self, Error> {
        let mcp_config = MCPConfig::from_json(mcp_config_json)?;
        let mut backends = Vec::new();
        for name in mcp_config.server_names() {
            let server = mcp_config
                .server(&name)
                .ok_or_else(|| Error::Config(format!("server not found: {name}")))?;
            backends.push(
                BackendServerConfig::new(name, server.command.clone(), server.args.clone())
                    .with_env(server.env.clone()),
            );
        }

        if backends.len() == 1 {
            let backend = backends.into_iter().next().expect("one backend exists");
            let public_name = config.server_name.clone().unwrap_or_default();
            let backend = connect_backend(
                backend,
                public_name,
                &config.include_tools,
                &config.exclude_tools,
            )
            .await?;
            Ok(Self {
                config,
                backends: vec![backend],
            })
        } else {
            Self::connect_multi_stdio(config, backends).await
        }
    }

    /// Start the frontend MCP server over streamable HTTP.
    pub async fn run_http(&self, _addr: SocketAddr) -> Result<RunningCompressedServer, Error> {
        todo!()
    }

    /// Return the frontend MCP tools exposed to callers.
    pub async fn list_frontend_tools(&self) -> Result<Vec<Tool>, Error> {
        if self.config.transform_mode == ProxyTransformMode::JustBash {
            return Ok(self.just_bash_tools());
        }
        if self.config.transform_mode == ProxyTransformMode::Cli {
            return Ok(self.cli_help_tools());
        }
        let mut tools = Vec::new();
        for backend in &self.backends {
            let prefix = self.wrapper_prefix(backend);
            tools.push(wrapper_tool(
                format!("{prefix}get_tool_schema"),
                "Return the full schema for a backend tool.",
            ));
            tools.push(wrapper_tool(
                format!("{prefix}invoke_tool"),
                "Invoke a backend tool by name.",
            ));
            if self.config.level == CompressionLevel::Max {
                tools.push(wrapper_tool(
                    format!("{prefix}list_tools"),
                    "List compressed backend tools.",
                ));
            }
        }
        Ok(tools)
    }

    /// Return the full backend schema for a tool via the compressed wrapper API.
    pub async fn get_tool_schema(
        &self,
        _wrapper_tool_name: &str,
        backend_tool_name: &str,
    ) -> Result<String, Error> {
        let backend = self.backend_for_wrapper(_wrapper_tool_name)?;
        let tool = backend
            .tools
            .iter()
            .find(|tool| tool.name == backend_tool_name)
            .ok_or_else(|| Error::ToolNotFound(backend_tool_name.to_string()))?;
        Ok(CompressionEngine::format_schema_response(tool))
    }

    /// List backend tools via the max-compression `list_tools` wrapper.
    pub async fn list_backend_tools(&self, wrapper_tool_name: &str) -> Result<String, Error> {
        let backend = self.backend_for_wrapper(wrapper_tool_name)?;
        let engine = CompressionEngine::new(CompressionLevel::High);
        Ok(engine
            .format_listing(&backend.tools)
            .lines()
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// Invoke a backend tool via the compressed wrapper API.
    pub async fn invoke_tool(
        &self,
        _wrapper_tool_name: &str,
        backend_tool_name: &str,
        tool_input: Value,
    ) -> Result<String, Error> {
        let backend = self.backend_for_wrapper(_wrapper_tool_name)?;
        self.invoke_backend(backend, backend_tool_name, tool_input)
            .await
    }

    /// List frontend resources, including pass-through backend resources and
    /// compressor-owned uncompressed-tool-list resources.
    pub async fn list_resources(&self) -> Result<Vec<String>, Error> {
        let mut resources = Vec::new();
        for backend in &self.backends {
            resources.extend(backend.resources.clone());
            resources.push(format!(
                "compressor://{}/uncompressed-tools",
                backend.public_name
            ));
        }
        Ok(resources)
    }

    /// Read a frontend resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<String, Error> {
        for backend in &self.backends {
            if uri == format!("compressor://{}/uncompressed-tools", backend.public_name) {
                return serde_json::to_string_pretty(&backend.tools).map_err(Error::from);
            }
        }
        let backend = self
            .backends
            .iter()
            .find(|backend| backend.resources.iter().any(|resource| resource == uri))
            .ok_or_else(|| Error::ToolNotFound(uri.to_string()))?;
        let result = backend
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(resource_contents_to_string(result.contents))
    }

    /// List frontend prompts passed through from backend servers.
    pub async fn list_prompts(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .backends
            .iter()
            .flat_map(|backend| backend.prompts.iter().map(|prompt| prompt.name.clone()))
            .collect())
    }

    /// Fetch a prompt from the backend that owns it.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Result<GetPromptResult, Error> {
        let backend = self
            .backends
            .iter()
            .find(|backend| backend.prompts.iter().any(|prompt| prompt.name == name))
            .ok_or_else(|| Error::ToolNotFound(name.to_string()))?;
        let mut request = GetPromptRequestParams::new(name);
        if let Some(arguments) = arguments {
            request = request.with_arguments(arguments);
        }
        backend
            .client
            .get_prompt(request)
            .await
            .map_err(|error| Error::Config(error.to_string()))
    }

    /// Return backend tools when the runtime has exactly one backend.
    pub fn single_backend_tools(&self) -> Result<Vec<Tool>, Error> {
        self.backends
            .first()
            .filter(|_| self.backends.len() == 1)
            .map(|backend| backend.tools.clone())
            .ok_or_else(|| Error::Config("expected exactly one backend".to_string()))
    }

    /// Invoke a backend tool directly when the runtime has exactly one backend.
    ///
    /// This is used by generated proxy clients, which call `/exec` with the
    /// backend tool name directly rather than the MCP wrapper tool name.
    pub fn just_bash_provider_specs(&self) -> Vec<JustBashProviderSpec> {
        self.backends
            .iter()
            .map(|backend| {
                let invoke_tool_name = format!("{}invoke_tool", self.wrapper_prefix(backend));
                JustBashProviderSpec {
                    provider_name: backend.public_name.clone(),
                    help_tool_name: format!("{}_help", backend.public_name),
                    tools: backend
                        .tools
                        .iter()
                        .map(|tool| JustBashCommandSpec {
                            command_name: crate::cli::mapping::tool_name_to_subcommand(&tool.name),
                            backend_tool_name: tool.name.clone(),
                            description: tool.description.clone(),
                            input_schema: tool.input_schema.clone(),
                            invoke_tool_name: invoke_tool_name.clone(),
                        })
                        .collect(),
                }
            })
            .collect()
    }

    pub async fn invoke_single_backend_tool(
        &self,
        backend_tool_name: &str,
        tool_input: Value,
    ) -> Result<String, Error> {
        let backend = self
            .backends
            .first()
            .filter(|_| self.backends.len() == 1)
            .ok_or_else(|| Error::ToolNotFound(backend_tool_name.to_string()))?;
        self.invoke_backend(backend, backend_tool_name, tool_input)
            .await
    }

    async fn invoke_backend(
        &self,
        backend: &ConnectedBackend,
        backend_tool_name: &str,
        tool_input: Value,
    ) -> Result<String, Error> {
        if !backend.tools.iter().any(|tool| tool.name == backend_tool_name) {
            return Err(Error::ToolNotFound(backend_tool_name.to_string()));
        }
        let arguments = match tool_input {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let mut params = CallToolRequestParams::new(backend_tool_name.to_string());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        let result = backend
            .client
            .call_tool(params)
            .await
            .map_err(|error| Error::Config(error.to_string()))?;
        let output = call_tool_result_to_string(result);
        Ok(self.maybe_toonify_output(&output))
    }

    fn maybe_toonify_output(&self, output: &str) -> String {
        if !self.config.toonify {
            return output.to_string();
        }
        let Ok(value) = serde_json::from_str::<Value>(output) else {
            return output.to_string();
        };
        toon_format::encode(&value, &toon_format::EncodeOptions::default())
            .unwrap_or_else(|_| output.to_string())
    }

    fn cli_help_tools(&self) -> Vec<Tool> {
        self.backends
            .iter()
            .map(|backend| {
                Tool::new(
                    format!("{}_help", backend.public_name),
                    Some(format_backend_help(backend)),
                    serde_json::json!({"type": "object", "properties": {}}),
                )
            })
            .collect()
    }

    fn just_bash_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        let names = self
            .backends
            .iter()
            .map(|backend| backend.public_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        tools.push(Tool::new(
            "bash_tool",
            Some(format!(
                "Register backend MCP tools as custom commands in a language-hosted just-bash instance. Providers: {names}. When relevant, prefer TOON output for compact representation."
            )),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Command text interpreted by the host language's just-bash implementation"}
                },
                "required": ["command"]
            }),
        ));
        tools.extend(self.cli_help_tools());
        tools
    }

    fn wrapper_prefix(&self, backend: &ConnectedBackend) -> String {
        if backend.public_name.is_empty() {
            String::new()
        } else {
            format!("{}_", backend.public_name)
        }
    }

    fn backend_for_wrapper(&self, wrapper_tool_name: &str) -> Result<&ConnectedBackend, Error> {
        if self.backends.len() == 1 && self.backends[0].public_name.is_empty() {
            return Ok(&self.backends[0]);
        }
        self.backends
            .iter()
            .find(|backend| wrapper_tool_name.starts_with(&self.wrapper_prefix(backend)))
            .ok_or_else(|| Error::ToolNotFound(wrapper_tool_name.to_string()))
    }
}

async fn connect_backend(
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

fn parse_http_backend_args(
    args: Vec<String>,
) -> (Vec<String>, HashMap<String, String>, BackendAuthMode) {
    let mut remaining = Vec::new();
    let mut headers = HashMap::new();
    let mut auth_mode = BackendAuthMode::Auto;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-H" || arg == "--header" {
            if let Some(header) = args.get(index + 1) {
                if let Some((name, value)) = parse_header_arg(header) {
                    headers.insert(name, value);
                } else {
                    remaining.push(arg.clone());
                    remaining.push(header.clone());
                }
                index += 2;
            } else {
                remaining.push(arg.clone());
                index += 1;
            }
        } else if let Some(mode) = arg.strip_prefix("--auth=") {
            match mode {
                "explicit-headers" | "headers" | "none" => {
                    auth_mode = BackendAuthMode::ExplicitHeaders;
                }
                "oauth" => {
                    auth_mode = BackendAuthMode::OAuth;
                }
                _ => remaining.push(arg.clone()),
            }
            index += 1;
        } else if arg == "--auth" {
            if let Some(mode) = args.get(index + 1) {
                match mode.as_str() {
                    "explicit-headers" | "headers" | "none" => {
                        auth_mode = BackendAuthMode::ExplicitHeaders;
                    }
                    "oauth" => {
                        auth_mode = BackendAuthMode::OAuth;
                    }
                    _ => {
                        remaining.push(arg.clone());
                        remaining.push(mode.clone());
                    }
                }
                index += 2;
            } else {
                remaining.push(arg.clone());
                index += 1;
            }
        } else if let Some(header) = arg
            .strip_prefix("-H=")
            .or_else(|| arg.strip_prefix("--header="))
        {
            if let Some((name, value)) = parse_header_arg(header) {
                headers.insert(name, value);
            } else {
                remaining.push(arg.clone());
            }
            index += 1;
        } else {
            remaining.push(arg.clone());
            index += 1;
        }
    }
    (remaining, headers, auth_mode)
}

fn parse_header_arg(header: &str) -> Option<(String, String)> {
    let (name, value) = header.split_once('=').or_else(|| header.split_once(':'))?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name.to_string(), interpolate_env(value)))
}

fn interpolate_env(value: &str) -> String {
    let mut output = String::new();
    let chars = value.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$' && chars.get(index + 1) == Some(&'{') {
            if let Some(end) = chars[index + 2..].iter().position(|ch| *ch == '}') {
                let name = chars[index + 2..index + 2 + end].iter().collect::<String>();
                output.push_str(&std::env::var(&name).unwrap_or_else(|_| format!("${{{name}}}")));
                index += end + 3;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn backend_http_headers(
    backend: &BackendServerConfig,
) -> Result<HashMap<HeaderName, HeaderValue>, Error> {
    backend
        .headers
        .iter()
        .map(|(name, value)| {
            let name = HeaderName::from_str(name).map_err(|error| {
                Error::Config(format!("invalid HTTP header name {name:?}: {error}"))
            })?;
            let value = HeaderValue::from_str(value).map_err(|error| {
                Error::Config(format!("invalid HTTP header value for {name:?}: {error}"))
            })?;
            Ok((name, value))
        })
        .collect()
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

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn convert_tool(tool: rmcp::model::Tool) -> Tool {
    Tool::new(
        tool.name.to_string(),
        tool.description.map(|description| description.to_string()),
        Value::Object((*tool.input_schema).clone()),
    )
}

fn format_backend_help(backend: &ConnectedBackend) -> String {
    let mut lines = vec![format!(
        "{} - the {} toolset",
        backend.public_name, backend.public_name
    )];
    lines.push(String::new());
    lines.push("When relevant, outputs from this CLI will prefer using the TOON format for more efficient representation of data.".to_string());
    lines.push(String::new());
    lines.push("SUBCOMMANDS:".to_string());
    for tool in &backend.tools {
        let subcommand = crate::cli::mapping::tool_name_to_subcommand(&tool.name);
        let description = tool.description.as_deref().unwrap_or_default();
        lines.push(format!("  {subcommand:<35} {description}"));
    }
    lines.join("\n")
}

fn wrapper_tool(name: String, description: &str) -> Tool {
    Tool::new(
        name,
        Some(description.to_string()),
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    )
}

fn call_tool_result_to_string(result: rmcp::model::CallToolResult) -> String {
    if let Some(structured) = result.structured_content {
        return value_to_string(&structured);
    }

    result
        .content
        .into_iter()
        .map(content_to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn content_to_string(content: Content) -> String {
    match content.raw {
        RawContent::Text(text) => text.text,
        RawContent::Image(image) => image.data,
        RawContent::Resource(resource) => resource_contents_to_string(vec![resource.resource]),
        RawContent::Audio(audio) => audio.data,
        RawContent::ResourceLink(resource) => resource.uri,
    }
}

fn resource_contents_to_string(contents: Vec<ResourceContents>) -> String {
    contents
        .into_iter()
        .map(|content| match content {
            ResourceContents::TextResourceContents { text, .. } => text,
            ResourceContents::BlobResourceContents { blob, .. } => blob,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Object(map) if map.len() == 1 && map.contains_key("result") => {
            value_to_string(&map["result"])
        }
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_backend_url_parses_curl_style_headers_after_separator() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token", "--header", "X-Test=yes"],
        );

        assert_eq!(backend.transport, BackendTransport::StreamableHttp);
        assert!(backend.args.is_empty());
        assert_eq!(backend.headers["Authorization"], "Basic token");
        assert_eq!(backend.headers["X-Test"], "yes");
    }

    #[test]
    fn http_backend_url_parses_equals_header_forms() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H=Authorization=Bearer token", "--header=X-Test=yes"],
        );

        assert!(backend.args.is_empty());
        assert_eq!(backend.headers["Authorization"], "Bearer token");
        assert_eq!(backend.headers["X-Test"], "yes");
    }

    #[test]
    fn http_backend_header_values_preserve_missing_environment_variables() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            [
                "-H",
                "Authorization=Bearer ${MCP_COMPRESSOR_MISSING_TEST_TOKEN}",
            ],
        );

        assert_eq!(
            backend.headers["Authorization"],
            "Bearer ${MCP_COMPRESSOR_MISSING_TEST_TOKEN}"
        );
    }

    #[test]
    fn remote_http_auto_auth_uses_oauth_without_authorization_header() {
        let backend =
            BackendServerConfig::new("remote", "https://example.test/mcp", [] as [&str; 0]);

        assert!(backend.should_use_oauth());
    }

    #[test]
    fn remote_http_auto_auth_skips_oauth_with_authorization_header() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token"],
        );

        assert!(backend.has_authorization_header());
        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn http_backend_url_parses_auth_mode_args() {
        let explicit = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["--auth", "explicit-headers"],
        );
        let oauth =
            BackendServerConfig::new("remote", "https://example.test/mcp", ["--auth=oauth"]);

        assert_eq!(explicit.auth_mode, BackendAuthMode::ExplicitHeaders);
        assert!(explicit.args.is_empty());
        assert_eq!(oauth.auth_mode, BackendAuthMode::OAuth);
        assert!(oauth.args.is_empty());
    }

    #[test]
    fn explicit_headers_auth_mode_skips_oauth_without_authorization_header() {
        let backend =
            BackendServerConfig::new("remote", "https://example.test/mcp", [] as [&str; 0])
                .with_auth_mode(BackendAuthMode::ExplicitHeaders);

        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn forced_oauth_auth_mode_uses_oauth_even_with_authorization_header() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token"],
        )
        .with_auth_mode(BackendAuthMode::OAuth);

        assert!(backend.should_use_oauth());
    }

    #[test]
    fn stdio_backend_never_uses_oauth() {
        let backend = BackendServerConfig::new("local", "python", ["server.py"]);

        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn http_backend_url_preserves_unrecognized_args_for_validation() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["--timeout", "30", "-H"],
        );

        assert_eq!(backend.args, ["--timeout", "30", "-H"]);
        assert!(backend.headers.is_empty());
    }
}
