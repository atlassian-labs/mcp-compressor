//! `CompressedServer` — the top-level object that owns the backend client,
//! tool cache, and compression engine, and exposes them via a frontend MCP server.
//!
//! This file exposes the high-level runtime API used by integration tests,
//! language bindings, and the standalone Rust CLI.

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CallToolResult, ClientRequest, ContentBlock,
    GetPromptRequest, GetPromptRequestParams, GetPromptResult, ReadResourceRequest,
    ReadResourceRequestParams, RequestMetaObject, ResourceContents, ServerResult,
};
use rmcp::service::PeerRequestOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;

pub(crate) const INVOKE_TOOL_INPUT_SCHEMA_DESCRIPTION: &str = concat!(
    "JSON object matching the selected backend tool's input schema. ",
    "Use get_tool_schema for the selected tool_name before invoking if required fields are unknown."
);

use crate::Error;
use crate::compression::CompressionLevel;
use crate::compression::engine::{CompressionEngine, Tool};
use crate::config::topology::MCPConfig;
use crate::server::backend::BackendServerConfig;
use crate::server::connect::{ConnectedBackend, backend_operation, connect_backend, timeout_error};

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

/// Connected compressor runtime.
#[derive(Debug)]
pub struct CompressedServer {
    config: CompressedServerConfig,
    backends: Vec<ConnectedBackend>,
}

impl CompressedServer {
    pub async fn shutdown(self) -> Result<(), Error> {
        self.shutdown_shared().await
    }

    /// Release every backend without consuming the server.
    ///
    /// SDK sessions are held behind an `Arc` that a draining HTTP bridge can
    /// still share, so shutdown must not require exclusive ownership.
    pub async fn shutdown_shared(&self) -> Result<(), Error> {
        let mut first_error = None;
        for backend in &self.backends {
            if let Err(error) = backend.shutdown_shared().await {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

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
        let backends = MCPConfig::from_json(mcp_config_json)?.into_backend_configs()?;

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
            tools.push(get_tool_schema_wrapper_tool(
                format!("{prefix}get_tool_schema"),
                &self.get_tool_schema_description(backend),
            ));
            tools.push(invoke_wrapper_tool(
                format!("{prefix}invoke_tool"),
                &self.invoke_tool_description(backend),
            ));
            if self.config.level == CompressionLevel::Max {
                tools.push(list_wrapper_tool(
                    format!("{prefix}list_tools"),
                    "List compressed backend tools.",
                ));
            }
        }
        Ok(tools)
    }

    fn get_tool_schema_description(&self, backend: &ConnectedBackend) -> String {
        format!(
            "Get the input schema for a specific tool from the {} toolset.\n\nAvailable tools are:\n{}",
            backend.public_name,
            self.frontend_tool_listing(backend)
        )
    }

    fn invoke_tool_description(&self, backend: &ConnectedBackend) -> String {
        format!(
            "Invoke a tool from the {} toolset. Use get_tool_schema first when you need the full input schema.",
            backend.public_name
        )
    }

    fn frontend_tool_listing(&self, backend: &ConnectedBackend) -> String {
        let listing = self.engine().format_listing(&backend.tools);
        if listing.is_empty() {
            backend
                .tools
                .iter()
                .map(|tool| format!("<tool>{}</tool>", tool.name))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            listing
        }
    }

    fn engine(&self) -> crate::compression::engine::CompressionEngine {
        crate::compression::engine::CompressionEngine::new(self.config.level.clone())
    }

    /// Return the default backend server name when a single unambiguous default exists.
    pub fn compression_level(&self) -> &CompressionLevel {
        &self.config.level
    }

    pub fn default_server_name(&self) -> Option<&str> {
        self.config.server_name.as_deref().or_else(|| {
            if self.backends.len() == 1 {
                Some(self.backends[0].public_name.as_str())
            } else {
                None
            }
        })
    }

    /// Return backend tool metadata for client generation and language bindings.
    pub fn backend_tools(&self) -> Vec<Tool> {
        self.backends
            .iter()
            .flat_map(|backend| backend.tools.iter().cloned())
            .collect()
    }

    /// Return backend tool metadata grouped by public backend server name.
    pub fn backend_tools_by_server(&self) -> Vec<(String, Tool)> {
        self.backends
            .iter()
            .flat_map(|backend| {
                backend
                    .tools
                    .iter()
                    .cloned()
                    .map(|tool| (backend.public_name.clone(), tool))
            })
            .collect()
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
        let result = self
            .invoke_backend_result(backend, backend_tool_name, tool_input, None)
            .await?;
        self.tool_result_to_output(result)
    }

    pub(crate) async fn invoke_tool_result(
        &self,
        wrapper_tool_name: &str,
        backend_tool_name: &str,
        tool_input: Value,
        request_meta: Option<RequestMetaObject>,
    ) -> Result<CallToolResult, Error> {
        let backend = self.backend_for_wrapper(wrapper_tool_name)?;
        let result = self
            .invoke_backend_result(backend, backend_tool_name, tool_input, request_meta)
            .await?;
        Ok(toonify_result(self.config.toonify, result))
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
        let result = match send_backend_request(
            backend,
            format!("read resource `{uri}`"),
            ClientRequest::ReadResourceRequest(ReadResourceRequest::new(
                ReadResourceRequestParams::new(uri),
            )),
        )
        .await?
        {
            ServerResult::ReadResourceResult(result) => result,
            _ => {
                return Err(Error::Config(
                    "unexpected read resource response".to_string(),
                ));
            }
        };
        resource_contents_to_string(result.contents)
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
        match send_backend_request(
            backend,
            format!("get prompt `{name}`"),
            ClientRequest::GetPromptRequest(GetPromptRequest::new(request)),
        )
        .await?
        {
            ServerResult::GetPromptResult(result) => Ok(result),
            _ => Err(Error::Config("unexpected get prompt response".to_string())),
        }
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
        let result = self
            .invoke_backend_result(backend, backend_tool_name, tool_input, None)
            .await?;
        self.tool_result_to_output(result)
    }

    async fn invoke_backend_result(
        &self,
        backend: &ConnectedBackend,
        backend_tool_name: &str,
        tool_input: Value,
        request_meta: Option<RequestMetaObject>,
    ) -> Result<CallToolResult, Error> {
        let tool = backend
            .tools
            .iter()
            .find(|tool| tool.name == backend_tool_name)
            .ok_or_else(|| Error::ToolNotFound(backend_tool_name.to_string()))?;
        validate_required_tool_input(tool, &tool_input)?;
        let arguments = match tool_input {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let mut params = CallToolRequestParams::new(backend_tool_name.to_string());
        params.meta = request_meta.map(backend_request_meta);
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        match send_backend_request(
            backend,
            format!("call tool `{backend_tool_name}`"),
            ClientRequest::CallToolRequest(CallToolRequest::new(params)),
        )
        .await?
        {
            ServerResult::CallToolResult(result) => Ok(result),
            _ => Err(Error::Config("unexpected call tool response".to_string())),
        }
    }

    /// Flatten a tool result for string transports (bridge `/exec`, in-process
    /// sessions). A result the backend flagged with `isError` becomes
    /// [`Error::ToolExecution`] so callers exit non-zero or raise, instead of
    /// printing the failure as if it were output.
    fn tool_result_to_output(&self, result: CallToolResult) -> Result<String, Error> {
        if result.is_error == Some(true) {
            return Err(Error::ToolExecution(call_tool_result_to_string(result)?));
        }
        let output = call_tool_result_to_string(result)?;
        Ok(self.maybe_toonify_output(&output))
    }

    fn maybe_toonify_output(&self, output: &str) -> String {
        toonify_output(self.config.toonify, output)
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
                "Register backend MCP tools as custom commands in a language-hosted just-bash instance. Providers: {names}."
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
        self.backends
            .iter()
            .find(|backend| {
                wrapper_tool_name
                    .strip_prefix(&self.wrapper_prefix(backend))
                    .is_some_and(|suffix| {
                        matches!(suffix, "get_tool_schema" | "invoke_tool" | "list_tools")
                    })
            })
            .ok_or_else(|| Error::ToolNotFound(wrapper_tool_name.to_string()))
    }
}

/// Remove frontend negotiation and unrelayed progress metadata.
fn backend_request_meta(mut meta: RequestMetaObject) -> RequestMetaObject {
    meta.remove("progressToken");
    meta.remove("io.modelcontextprotocol/protocolVersion");
    meta.remove("io.modelcontextprotocol/clientInfo");
    meta.remove("io.modelcontextprotocol/clientCapabilities");
    meta
}

#[cfg(test)]
mod request_timeout_tests {
    use super::*;
    use futures::FutureExt;
    use std::time::Duration;

    enum QueueState {
        FullBeforeSubmission,
        FullAfterSubmission,
        Available,
    }

    #[test]
    fn stdio_timeout_bounds_submission_to_a_full_rmcp_queue() {
        assert_request_deadline(QueueState::FullBeforeSubmission);
    }

    #[test]
    fn timeout_bounds_cancellation_queue_wait() {
        assert_request_deadline(QueueState::FullAfterSubmission);
    }

    #[test]
    fn timeout_bounds_cancellation_transport_acknowledgement() {
        assert_request_deadline(QueueState::Available);
    }

    fn fill_peer_queue(backend: &ConnectedBackend) {
        loop {
            let submitted = tokio::task::unconstrained(backend.client.send_cancellable_request(
                ClientRequest::PingRequest(Default::default()),
                PeerRequestOptions::default(),
            ))
            .now_or_never();
            match submitted {
                Some(Ok(_)) => {}
                Some(Err(error)) => panic!("submission failed before saturation: {error}"),
                None => break,
            }
        }
    }

    fn assert_request_deadline(queue_state: QueueState) {
        let service_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let caller_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (transport, _remote) = tokio::io::duplex(1024);
        let client = {
            let _entered = service_runtime.enter();
            rmcp::service::serve_directly((), transport, None)
        };
        let backend =
            ConnectedBackend::for_test("blocked", client, Some(Duration::from_millis(50)));
        let result = caller_runtime.block_on(async {
            // The service runtime stays paused to control queue and transport progress.
            if matches!(queue_state, QueueState::FullBeforeSubmission) {
                fill_peer_queue(&backend);
            }
            let request = send_backend_request(
                &backend,
                "ping".into(),
                ClientRequest::PingRequest(Default::default()),
            );
            tokio::pin!(request);
            assert!(
                tokio::task::unconstrained(request.as_mut())
                    .now_or_never()
                    .is_none()
            );
            if matches!(queue_state, QueueState::FullAfterSubmission) {
                fill_peer_queue(&backend);
            }
            tokio::time::timeout(Duration::from_millis(500), request).await
        });
        service_runtime.block_on(backend.shutdown_shared()).unwrap();
        let error = result
            .expect("request or cancellation ignored the configured deadline")
            .unwrap_err();
        assert!(matches!(error, Error::Config(_)), "{error}");
    }
}

async fn send_backend_request(
    backend: &ConnectedBackend,
    operation: String,
    request: ClientRequest,
) -> Result<ServerResult, Error> {
    // One deadline covers submission and response; cancellation has bounded cleanup grace.
    let deadline = backend.timeout.map(|timeout| Instant::now() + timeout);
    let submit = async {
        let mut options = PeerRequestOptions::default();
        options.timeout = backend.timeout;
        backend
            .client
            .send_cancellable_request(request, options)
            .await
            .map_err(|error| Error::Config(error.to_string()))
    };
    let mut handle =
        backend_operation(backend.timeout, &backend.backend_name, &operation, submit).await?;
    let response = if let (Some(deadline), Some(timeout)) = (deadline, backend.timeout) {
        handle.options.timeout = Some(
            deadline
                .saturating_duration_since(Instant::now())
                .min(timeout),
        );
        let cancellation_deadline = deadline + std::time::Duration::from_millis(100);
        match tokio::time::timeout_at(cancellation_deadline, handle.await_response()).await {
            Ok(response) => response,
            Err(_) => {
                eprintln!(
                    "timed-out backend {} request {operation}: cancellation exceeded 100ms cleanup grace",
                    backend.backend_name
                );
                return Err(timeout_error(&backend.backend_name, &operation, timeout));
            }
        }
    } else {
        handle.await_response().await
    };
    match response {
        Ok(response) => Ok(response),
        Err(rmcp::service::ServiceError::Timeout { .. }) => Err(timeout_error(
            &backend.backend_name,
            &operation,
            backend.timeout.unwrap_or_default(),
        )),
        Err(error) => Err(Error::Config(error.to_string())),
    }
}

fn validate_required_tool_input(tool: &Tool, tool_input: &Value) -> Result<(), Error> {
    let required = required_field_names(&tool.input_schema);
    if required.is_empty() {
        return Ok(());
    }

    let input = match tool_input.as_object() {
        Some(input) => input,
        None => return Err(missing_required_tool_input_error(tool, &required)),
    };
    let missing = required
        .iter()
        .filter(|field| !input.contains_key(field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing_required_tool_input_error(tool, &missing))
    }
}

fn required_field_names(input_schema: &Value) -> Vec<String> {
    input_schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn missing_required_tool_input_error(tool: &Tool, missing: &[String]) -> Error {
    let schema = serde_json::to_string_pretty(&tool.input_schema)
        .unwrap_or_else(|_| tool.input_schema.to_string());
    Error::Validation(format!(
        "Tool `{}` is missing required tool_input fields: {}. tool_input must be a JSON object matching the selected backend tool's input schema. Call get_tool_schema with tool_name=`{}` and retry with tool_input matching this schema:\n{}",
        tool.name,
        missing.join(", "),
        tool.name,
        schema
    ))
}

/// The `<server>_help` tool description: the same top-level help the generated
/// CLI prints, framed to steer the model to the command instead of the tool.
/// Shares the renderer with the FFI host transforms so all surfaces match.
fn format_backend_help(backend: &ConnectedBackend) -> String {
    let command = backend.public_name.as_str();
    crate::cli::help::render_top_level_help(
        command,
        command,
        &backend.tools,
        &crate::cli::help::HelpFraming::help_tool(command, command),
    )
}

pub(crate) fn get_tool_schema_wrapper_tool(name: String, description: &str) -> Tool {
    Tool::new(
        name,
        Some(description.to_string()),
        serde_json::json!({
            "type": "object",
            "properties": {
                "tool_name": { "type": "string", "description": "Name of the backend tool" }
            },
            "required": ["tool_name"]
        }),
    )
}

pub(crate) fn invoke_wrapper_tool(name: String, description: &str) -> Tool {
    Tool::new(
        name,
        Some(description.to_string()),
        serde_json::json!({
            "type": "object",
            "properties": {
                "tool_name": { "type": "string", "description": "Name of the backend tool" },
                "tool_input": {
                    "type": "object",
                    "description": INVOKE_TOOL_INPUT_SCHEMA_DESCRIPTION,
                    "properties": {},
                    "additionalProperties": true
                }
            },
            "required": ["tool_name", "tool_input"]
        }),
    )
}

pub(crate) fn list_wrapper_tool(name: String, description: &str) -> Tool {
    Tool::new(
        name,
        Some(description.to_string()),
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    )
}

fn call_tool_result_to_string(result: rmcp::model::CallToolResult) -> Result<String, Error> {
    if let Some(structured) = result.structured_content {
        return Ok(value_to_string(&structured));
    }

    Ok(result
        .content
        .into_iter()
        .map(content_to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n"))
}

fn content_to_string(content: ContentBlock) -> Result<String, Error> {
    Ok(match content {
        ContentBlock::Text(text) => text.text,
        ContentBlock::Image(image) => image.data,
        ContentBlock::Resource(resource) => resource_contents_to_string(vec![resource.resource])?,
        ContentBlock::Audio(audio) => audio.data,
        ContentBlock::ResourceLink(resource) => resource.uri,
        other => serde_json::to_string(&other)?,
    })
}

fn resource_contents_to_string(contents: Vec<ResourceContents>) -> Result<String, Error> {
    Ok(contents
        .into_iter()
        .map(|content| match content {
            ResourceContents::TextResourceContents { text, .. } => Ok(text),
            ResourceContents::BlobResourceContents { blob, .. } => Ok(blob),
            other => serde_json::to_string(&other),
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\n"))
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

pub(crate) fn toonify_output(toonify: bool, output: &str) -> String {
    if !toonify {
        return output.to_string();
    }
    let Some(value) = parse_structured_output(output) else {
        return output.to_string();
    };
    toon_format::encode(&value, &toon_format::EncodeOptions::default())
        .unwrap_or_else(|_| output.to_string())
}

/// YAML goes before CSV, or `tags: a,b` would read as a two-column table.
fn parse_structured_output(output: &str) -> Option<Value> {
    let text = output.strip_prefix('\u{feff}').unwrap_or(output);
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Some(value);
    }
    let yaml = yaml_candidate(text)
        .then(|| yaml_serde::from_str(text).ok())
        .flatten()
        .and_then(yaml_to_json);
    match yaml {
        Some(value @ (Value::Object(_) | Value::Array(_))) => {
            yaml_is_structured(&value).then_some(value)
        }
        _ => csv_to_json(text),
    }
}

/// libyaml copies every alias (a few KB of anchors can grow to gigabytes) and
/// drops comments (`tags: #ai` becomes null), so such text stays verbatim.
fn yaml_candidate(text: &str) -> bool {
    !(text.contains('&') && text.contains('*'))
        && !text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with('#') || line.contains(" #") || line.contains("\t#")
        })
}

/// Going through `yaml_serde::Value` rejects duplicate keys, which a
/// `serde_json::Value` target would silently collapse to the last entry.
fn yaml_to_json(value: yaml_serde::Value) -> Option<Value> {
    use yaml_serde::Value as Yaml;
    Some(match value {
        Yaml::Null => Value::Null,
        Yaml::Bool(flag) => Value::Bool(flag),
        Yaml::Number(number) => serde_json::from_str(&number.to_string()).ok()?,
        Yaml::String(text) => Value::String(text),
        Yaml::Sequence(items) => {
            Value::Array(items.into_iter().map(yaml_to_json).collect::<Option<_>>()?)
        }
        Yaml::Mapping(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| match key {
                    Yaml::String(key) => Some((key, yaml_to_json(value)?)),
                    _ => None,
                })
                .collect::<Option<_>>()?,
        ),
        Yaml::Tagged(_) => return None,
    })
}

/// Prose like "Found 3 files:\n- a.txt" is valid YAML too, and a flat
/// `key: value` block reads the same in TOON, so only real records convert.
fn yaml_is_structured(value: &Value) -> bool {
    fn is_record(map: &serde_json::Map<String, Value>) -> bool {
        map.len() >= 2 && map.keys().all(|key| !key.contains(char::is_whitespace))
    }
    fn has_record(value: &Value) -> bool {
        match value {
            Value::Object(map) => is_record(map) || map.values().any(has_record),
            Value::Array(items) => items.iter().any(has_record),
            _ => false,
        }
    }
    match value {
        Value::Object(map) => {
            map.keys().all(|key| !key.contains(char::is_whitespace))
                && map.values().any(|value| match value {
                    Value::Object(map) => !map.is_empty(),
                    Value::Array(items) => !items.is_empty(),
                    _ => false,
                })
                && has_record(value)
        }
        Value::Array(items) => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| item.as_object().is_some_and(is_record))
        }
        _ => false,
    }
}

/// Headers must look like column names, so logs, `KEY=a,b` lines and prose
/// stay out; rows of a different width fail the non-flexible reader. A
/// single row of plain words under plain-word headers ("Hello,world\nFoo,bar")
/// reads as two lines of prose, so one row must hold a number, boolean or
/// empty cell to count as data.
fn csv_to_json(text: &str) -> Option<Value> {
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader.headers().ok()?.clone();
    let named = headers.len() >= 2
        && headers.iter().all(|cell| {
            cell.trim() == cell
                && cell.starts_with(|c: char| c.is_alphabetic() || c == '_')
                && cell
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
        })
        && headers
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == headers.len();
    if !named {
        return None;
    }
    let rows = reader
        .records()
        .map(|record| {
            let record = record.ok()?;
            Some(Value::Object(
                headers
                    .iter()
                    .map(String::from)
                    .zip(record.iter().map(csv_cell))
                    .collect(),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let looks_like_data = match rows.as_slice() {
        [] => false,
        [row] => row.as_object().is_some_and(|cells| {
            cells
                .values()
                .any(|cell| !cell.is_string() || cell.as_str() == Some(""))
        }),
        _ => true,
    };
    looks_like_data.then_some(Value::Array(rows))
}

/// Typed only when it prints back as the same text, so "007" and "3.10" survive.
fn csv_cell(cell: &str) -> Value {
    let int = cell
        .parse::<i64>()
        .ok()
        .filter(|int| int.to_string() == cell);
    let float = cell
        .parse::<f64>()
        .ok()
        .filter(|float| cell.contains('.') && float.to_string() == cell);
    match cell {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => int
            .map(Value::from)
            .or_else(|| float.map(Value::from))
            .unwrap_or_else(|| Value::String(cell.to_string())),
    }
}

/// Apply `--toonify` to a pass-through tool result.
///
/// The MCP frontend returns the backend `CallToolResult` verbatim to preserve
/// structured and typed content, so the TOON re-encoding has to happen here
/// instead of in the string-returning path. Only text blocks that parse as
/// JSON, a CSV table or structured YAML change; typed content, structured
/// content and error results are left untouched.
fn toonify_result(toonify: bool, mut result: CallToolResult) -> CallToolResult {
    if !toonify || result.is_error == Some(true) {
        return result;
    }
    for item in result.content.iter_mut() {
        if let ContentBlock::Text(text) = item {
            text.text = toonify_output(true, &text.text);
        }
    }
    result
}

#[cfg(test)]
mod toonify_tests {
    use super::*;

    fn json_result() -> CallToolResult {
        serde_json::from_value(serde_json::json!({
            "content": [{"type": "text", "text": "[{\"id\":1,\"name\":\"alpha\"}]"}]
        }))
        .unwrap()
    }

    /// The MCP frontend returns backend results verbatim, so `--toonify`
    /// must still be applied there or the flag silently does nothing.
    #[test]
    fn toonify_reencodes_json_text_blocks_of_passthrough_results() {
        let result = toonify_result(true, json_result());

        let text = match &result.content[0] {
            ContentBlock::Text(text) => text.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert_ne!(text, "[{\"id\":1,\"name\":\"alpha\"}]");
        assert_eq!(
            text,
            toonify_output(true, "[{\"id\":1,\"name\":\"alpha\"}]")
        );
    }

    /// Without the flag the result must stay byte-identical.
    #[test]
    fn toonify_disabled_preserves_passthrough_results() {
        let result = toonify_result(false, json_result());

        let text = match &result.content[0] {
            ContentBlock::Text(text) => text.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert_eq!(text, "[{\"id\":1,\"name\":\"alpha\"}]");
    }

    /// Backend failures must not be re-encoded; agents rely on the raw text.
    #[test]
    fn toonify_preserves_backend_error_results() {
        let mut result = json_result();
        result.is_error = Some(true);

        let result = toonify_result(true, result);

        let text = match &result.content[0] {
            ContentBlock::Text(text) => text.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert_eq!(text, "[{\"id\":1,\"name\":\"alpha\"}]");
    }

    /// CSV, YAML and JSON take the same TOON form; CSV cells and YAML
    /// scalars keep digits such as `007` and `3.10`.
    #[test]
    fn toonify_converts_structured_text() {
        for (text, expected) in [
            ("id,name\n1,alpha\n2,beta\n", "[2]{id,name}:\n  1,alpha\n  2,beta"),
            ("\u{feff}id,name\r\n1,alpha\r\n", "[1]{id,name}:\n  1,alpha"),
            ("id,note\n1,\"a, b\nc\"\n", "[1]{id,note}:\n  1,\"a, b\\nc\""),
            (
                "id,zip,ver,neg,exp,ratio,flag\n42,007,3.10,-0,1e5,2.5,true\n",
                "[1]{id,zip,ver,neg,exp,ratio,flag}:\n  42,\"007\",\"3.10\",\"-0\",\"1e5\",2.5,true",
            ),
            ("a,b\n1,\n", "[1]{a,b}:\n  1,\"\""),
            // Several all-text rows are still a table.
            (
                "name,team\nada,core\nlin,web\n",
                "[2]{name,team}:\n  ada,core\n  lin,web",
            ),
            ("name: svc\nports:\n  - 80\n  - 443\n", "name: svc\nports[2]: 80,443"),
            (
                "- id: 1\n  name: alpha\n- id: 2\n  name: beta\n",
                "[2]{id,name}:\n  1,alpha\n  2,beta",
            ),
            ("results:\n  - id: 1\n    name: x\n", "results[1]{id,name}:\n  1,x"),
            (
                "flags:\n  enabled: yes\n  zip: 007\n  when: 2026-09-22\n  big: 1_000\n",
                "flags:\n  enabled: yes\n  zip: \"007\"\n  when: \"2026-09-22\"\n  big: 1_000",
            ),
            // Every JSON document is valid YAML, so JSON must win, BOM or not.
            ("[{\"id\":1,\"name\":\"alpha\"}]", "[1]{id,name}:\n  1,alpha"),
            ("\u{feff}{\"a\":[1,2]}", "a[2]: 1,2"),
        ] {
            assert_eq!(toonify_output(true, text), expected, "input {text:?}");
        }
    }

    #[test]
    fn toonify_disabled_leaves_csv_and_yaml_untouched() {
        for text in ["id,name\n1,alpha\n", "name: svc\nports:\n  - 80\n"] {
            assert_eq!(toonify_output(false, text), text);
        }
    }

    /// Prose, logs, flat `key: value` blocks and bullet lists are valid YAML
    /// or merely contain commas, and YAML with aliases, comments, duplicate
    /// keys, tags or `.inf` would lose or invent data; none may be rewritten.
    #[test]
    fn toonify_leaves_prose_untouched() {
        for text in [
            "Hello, world",
            "Hello, world\nGoodbye, moon\n",
            "Error: connection refused",
            "Status: ok\nUptime: 3d\nPython: 3.10\n",
            "Version: 3.10\nBuild: 42\n",
            "Content-Type: text/plain\nContent-Length: 12\n",
            "## Steps\n- open the file\n- save it\n",
            "Steps:\n- open the file\n- save it\n",
            "Found 3 files:\n- a.txt\n- b.txt\n",
            "Errors:\n- foo failed\n- bar failed\n",
            "- Note: do this\n- Tip: do that\n",
            "Plan:\n- step one\n  - sub a\n- step two\n",
            "Total: 1,234 items\nSubtotal: 1,200 items\n",
            "tags: a,b\nids: 1,2\n",
            "2026-09-22 10:00:00 INFO started\n2026-09-22 10:00:01 INFO done\n",
            "2026-09-22,INFO,started\n2026-09-22,INFO,done\n",
            "1,alpha\n2,beta\n",
            "Hello,world\nFoo,bar",
            "Yes,thanks\nSee,you\n",
            "id,name\n",
            "id,name\n1,alpha,extra\n",
            "id\n1\n2\n",
            "a,,c\n1,2,3\n",
            "a,b,a\n1,2,3\n",
            "id,name \n1,alpha\n",
            "Traceback (most recent call last):\n  File \"x.py\", line 1, in <module>\nValueError: bad\n",
            "| id | name |\n|---|---|\n| 1 | a |\n",
            "---\na:\n  b: 1\n---\nc:\n  d: 2\n",
            "{\"a\":1}\n{\"a\":2}",
            "a: !custom 1\nb:\n  c: 2\n",
            "base: &b\n  x: 1\nother: *b\n",
            "post:\n  title: Hello\n  tags: #ai #ml\n",
            "event: message\ndata: {\"id\": 1}\n\nevent: message\ndata: {\"id\": 2}\n",
            "stats:\n  max: .inf\n  min: 0\nitems:\n  - id: 1\n    ok: true\n",
            "ALLOWED_HOSTS=a,b\nPORTS=80,443\n",
            "host=x\nport=80\n",
            "On branch main\nYour branch is up to date with 'origin/main'.\n",
            "Description:\n  This is a long\n  multi-line note.\nStatus: ok\n",
            "alpha:filtered",
            "",
            "  \n",
        ] {
            assert_eq!(toonify_output(true, text), text, "rewrote {text:?}");
        }
    }

    #[test]
    fn toonify_reencodes_csv_text_blocks_of_passthrough_results() {
        let result: CallToolResult = serde_json::from_value(serde_json::json!({
            "content": [{"type": "text", "text": "id,name\n1,alpha\n"}]
        }))
        .unwrap();

        let result = toonify_result(true, result);

        let text = match &result.content[0] {
            rmcp::model::ContentBlock::Text(text) => text.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert_eq!(text, "[1]{id,name}:\n  1,alpha");
    }
}
