//! `CompressedServer` — the top-level object that owns the backend client,
//! tool cache, and compression engine, and exposes them via a frontend MCP server.
//!
//! This file exposes the high-level runtime API used by integration tests,
//! language bindings, and the standalone Rust CLI.

use rmcp::model::{
    CallToolRequestParams, Content, GetPromptRequestParams, GetPromptResult, RawContent,
    ReadResourceRequestParams, ResourceContents,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compression::engine::{CompressionEngine, Tool};
use crate::compression::CompressionLevel;
use crate::config::topology::MCPConfig;
use crate::server::backend::BackendServerConfig;
use crate::server::connect::{connect_backend, ConnectedBackend};
use crate::Error;

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
        if !backend
            .tools
            .iter()
            .any(|tool| tool.name == backend_tool_name)
        {
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
