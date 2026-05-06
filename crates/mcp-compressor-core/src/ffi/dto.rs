use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client_gen::generator::GeneratorConfig;
use crate::compression::engine::Tool;
use crate::server::{
    BackendServerConfig, JustBashCommandSpec, JustBashProviderSpec,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfiTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FfiMcpServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cli_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfiJustBashProviderSpec {
    pub provider_name: String,
    pub help_tool_name: String,
    pub tools: Vec<FfiJustBashCommandSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfiJustBashCommandSpec {
    pub command_name: String,
    pub backend_tool_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub invoke_tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfiGeneratorConfig {
    pub cli_name: String,
    pub bridge_url: String,
    pub token: String,
    pub tools: Vec<FfiTool>,
    pub session_pid: u32,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FfiBackendConfig {
    pub name: String,
    pub command_or_url: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FfiCompressedSessionConfig {
    pub compression_level: String,
    pub server_name: Option<String>,
    #[serde(default)]
    pub include_tools: Vec<String>,
    #[serde(default)]
    pub exclude_tools: Vec<String>,
    #[serde(default)]
    pub toonify: bool,
    pub transform_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfiCompressedSessionInfo {
    pub bridge_url: String,
    pub token: String,
    pub frontend_tools: Vec<FfiTool>,
    pub just_bash_providers: Vec<FfiJustBashProviderSpec>,
}

impl From<FfiBackendConfig> for BackendServerConfig {
    fn from(value: FfiBackendConfig) -> Self {
        BackendServerConfig::new(value.name, value.command_or_url, value.args)
    }
}

impl From<FfiGeneratorConfig> for GeneratorConfig {
    fn from(value: FfiGeneratorConfig) -> Self {
        Self {
            cli_name: value.cli_name,
            bridge_url: value.bridge_url,
            token: value.token,
            tools: value.tools.into_iter().map(Into::into).collect(),
            session_pid: value.session_pid,
            output_dir: value.output_dir,
        }
    }
}

impl From<FfiTool> for Tool {
    fn from(value: FfiTool) -> Self {
        Tool::new(value.name, value.description, value.input_schema)
    }
}

impl From<Tool> for FfiTool {
    fn from(value: Tool) -> Self {
        Self {
            name: value.name,
            description: value.description,
            input_schema: value.input_schema,
        }
    }
}

impl From<JustBashProviderSpec> for FfiJustBashProviderSpec {
    fn from(value: JustBashProviderSpec) -> Self {
        Self {
            provider_name: value.provider_name,
            help_tool_name: value.help_tool_name,
            tools: value.tools.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<JustBashCommandSpec> for FfiJustBashCommandSpec {
    fn from(value: JustBashCommandSpec) -> Self {
        Self {
            command_name: value.command_name,
            backend_tool_name: value.backend_tool_name,
            description: value.description,
            input_schema: value.input_schema,
            invoke_tool_name: value.invoke_tool_name,
        }
    }
}
