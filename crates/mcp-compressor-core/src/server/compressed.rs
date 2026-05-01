//! `CompressedServer` — the top-level object that owns the backend client,
//! tool cache, and compression engine, and exposes them via a frontend MCP server.
//!
//! This file intentionally exposes the runtime API that integration tests and
//! language bindings should target. Method bodies remain `todo!()` until the
//! Phase 1 runtime is implemented.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Stdio;

use rmcp::model::{
    CallToolRequestParams, Content, RawContent, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

use crate::compression::engine::{CompressionEngine, Tool};
use crate::compression::CompressionLevel;
use crate::Error;

/// Configuration for one upstream MCP server process reached over stdio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

impl BackendServerConfig {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: HashMap::new(),
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        self
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
    backend_name: String,
    client: RunningService<RoleClient, ()>,
    tools: Vec<Tool>,
    resources: Vec<String>,
    prompts: Vec<String>,
}

impl CompressedServer {
    /// Connect to one upstream stdio MCP server.
    pub async fn connect_stdio(
        config: CompressedServerConfig,
        backend: BackendServerConfig,
    ) -> Result<Self, Error> {
        let mut command = tokio::process::Command::new(&backend.command);
        command
            .args(&backend.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        command.stderr(Stdio::inherit());
        for (key, value) in &backend.env {
            command.env(key, value);
        }

        let transport = TokioChildProcess::new(command.configure(|_| {}))
            .map_err(|error| Error::Io(error))?;
        let client = ()
            .serve(transport)
            .await
            .map_err(|error| Error::Config(error.to_string()))?;

        let rmcp_tools = client
            .list_all_tools()
            .await
            .map_err(|error| Error::Config(error.to_string()))?;
        let tools = rmcp_tools.into_iter().map(convert_tool).collect::<Vec<_>>();

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
        let prompts = client
            .list_all_prompts()
            .await
            .map(|prompts| prompts.into_iter().map(|prompt| prompt.name).collect::<Vec<_>>())
            .unwrap_or_default();

        Ok(Self {
            config,
            backend_name: backend.name,
            client,
            tools,
            resources,
            prompts,
        })
    }

    /// Connect to multiple upstream stdio MCP servers.
    pub async fn connect_multi_stdio(
        _config: CompressedServerConfig,
        _backends: Vec<BackendServerConfig>,
    ) -> Result<Self, Error> {
        todo!()
    }

    /// Connect using a JSON MCP config document containing one or more `mcpServers` entries.
    pub async fn connect_mcp_config_json(
        _config: CompressedServerConfig,
        _mcp_config_json: &str,
    ) -> Result<Self, Error> {
        todo!()
    }

    /// Start the frontend MCP server over streamable HTTP.
    pub async fn run_http(&self, _addr: SocketAddr) -> Result<RunningCompressedServer, Error> {
        todo!()
    }

    /// Return the frontend MCP tools exposed to callers.
    pub async fn list_frontend_tools(&self) -> Result<Vec<Tool>, Error> {
        let prefix = self.wrapper_prefix();
        let mut tools = vec![
            wrapper_tool(
                format!("{prefix}get_tool_schema"),
                "Return the full schema for a backend tool.",
            ),
            wrapper_tool(format!("{prefix}invoke_tool"), "Invoke a backend tool by name."),
        ];
        if self.config.level == CompressionLevel::Max {
            tools.push(wrapper_tool(
                format!("{prefix}list_tools"),
                "List compressed backend tools.",
            ));
        }
        Ok(tools)
    }

    /// Return the full backend schema for a tool via the compressed wrapper API.
    pub async fn get_tool_schema(
        &self,
        _wrapper_tool_name: &str,
        backend_tool_name: &str,
    ) -> Result<String, Error> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name == backend_tool_name)
            .ok_or_else(|| Error::ToolNotFound(backend_tool_name.to_string()))?;
        Ok(CompressionEngine::format_schema_response(tool))
    }

    /// List backend tools via the max-compression `list_tools` wrapper.
    pub async fn list_backend_tools(&self, _wrapper_tool_name: &str) -> Result<String, Error> {
        let engine = CompressionEngine::new(CompressionLevel::High);
        Ok(engine
            .format_listing(&self.tools)
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
        let arguments = match tool_input {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let mut params = CallToolRequestParams::new(backend_tool_name.to_string());
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        let result = self
            .client
            .call_tool(params)
            .await
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(call_tool_result_to_string(result))
    }

    /// List frontend resources, including pass-through backend resources and
    /// compressor-owned uncompressed-tool-list resources.
    pub async fn list_resources(&self) -> Result<Vec<String>, Error> {
        let mut resources = self.resources.clone();
        resources.push(format!(
            "compressor://{}/uncompressed-tools",
            self.public_server_name()
        ));
        Ok(resources)
    }

    /// Read a frontend resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<String, Error> {
        if uri == format!("compressor://{}/uncompressed-tools", self.public_server_name()) {
            return serde_json::to_string_pretty(&self.tools).map_err(Error::from);
        }
        let result = self
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(resource_contents_to_string(result.contents))
    }

    /// List frontend prompts passed through from backend servers.
    pub async fn list_prompts(&self) -> Result<Vec<String>, Error> {
        Ok(self.prompts.clone())
    }

    fn public_server_name(&self) -> &str {
        self.config.server_name.as_deref().unwrap_or(&self.backend_name)
    }

    fn wrapper_prefix(&self) -> String {
        format!("{}_", self.public_server_name())
    }
}

fn convert_tool(tool: rmcp::model::Tool) -> Tool {
    Tool::new(
        tool.name.to_string(),
        tool.description.map(|description| description.to_string()),
        Value::Object((*tool.input_schema).clone()),
    )
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
