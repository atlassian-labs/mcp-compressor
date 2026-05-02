//! JSON-serializable DTOs and helpers for PyO3 / napi-rs language bindings.
//!
//! These are not a C ABI. They are intentionally plain Rust data-transfer
//! objects that binding crates can expose idiomatically in Python and
//! TypeScript while sharing the same core behavior.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::parser::parse_argv;
use crate::client_gen::cli::CliGenerator;
use crate::client_gen::generator::{ClientGenerator, GeneratorConfig};
use crate::client_gen::python::PythonGenerator;
use crate::client_gen::typescript::TypeScriptGenerator;
use crate::compression::engine::{CompressionEngine, Tool};
use crate::compression::CompressionLevel;
use crate::config::topology::MCPConfig;
use crate::server::{JustBashCommandSpec, JustBashProviderSpec};
use crate::Error;

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

pub fn compress_tool_listing(level: CompressionLevel, tools: Vec<FfiTool>) -> String {
    let tools = tools.into_iter().map(Into::into).collect::<Vec<Tool>>();
    CompressionEngine::new(level).format_listing(&tools)
}

pub fn format_tool_schema_response(tool: FfiTool) -> String {
    CompressionEngine::format_schema_response(&tool.into())
}

pub fn parse_tool_argv(tool: FfiTool, argv: Vec<String>) -> Result<Value, Error> {
    parse_argv(&argv, &tool.into())
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FfiClientArtifactKind {
    Cli,
    Python,
    TypeScript,
}

pub fn generate_client_artifacts(
    kind: FfiClientArtifactKind,
    config: FfiGeneratorConfig,
) -> Result<Vec<PathBuf>, Error> {
    let config = GeneratorConfig::from(config);
    match kind {
        FfiClientArtifactKind::Cli => CliGenerator.generate(&config),
        FfiClientArtifactKind::Python => PythonGenerator.generate(&config),
        FfiClientArtifactKind::TypeScript => TypeScriptGenerator.generate(&config),
    }
}

pub fn parse_mcp_config(config_json: &str) -> Result<Vec<FfiMcpServer>, Error> {
    let config = MCPConfig::from_json(config_json)?;
    Ok(config
        .server_names()
        .into_iter()
        .filter_map(|name| {
            let server = config.server(&name)?;
            Some(FfiMcpServer {
                cli_prefix: config.cli_prefix(&name),
                name,
                command: server.command.clone(),
                args: server.args.clone(),
                env: server
                    .env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            })
        })
        .collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool() -> FfiTool {
        FfiTool {
            name: "echo".to_string(),
            description: Some("Echo a message.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            }),
        }
    }

    #[test]
    fn ffi_compresses_tool_listing() {
        let listing = compress_tool_listing(CompressionLevel::High, vec![sample_tool()]);
        assert_eq!(listing, "<tool>echo(message)</tool>");
    }

    #[test]
    fn ffi_formats_schema_response() {
        let schema = format_tool_schema_response(sample_tool());
        assert!(schema.contains("Echo a message."));
        assert!(schema.contains("message"));
    }

    #[test]
    fn ffi_parses_tool_argv() {
        let parsed = parse_tool_argv(
            sample_tool(),
            vec!["--message".to_string(), "hello".to_string()],
        )
        .unwrap();
        assert_eq!(parsed, json!({ "message": "hello" }));
    }

    fn generator_config(output_dir: &std::path::Path) -> FfiGeneratorConfig {
        FfiGeneratorConfig {
            cli_name: "ffi-server".to_string(),
            bridge_url: "http://127.0.0.1:12345".to_string(),
            token: "token".repeat(16),
            tools: vec![sample_tool()],
            session_pid: 42,
            output_dir: output_dir.to_path_buf(),
        }
    }

    #[test]
    fn ffi_generates_cli_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths =
            generate_client_artifacts(FfiClientArtifactKind::Cli, generator_config(dir.path()))
                .unwrap();
        assert_eq!(paths.len(), 1);
        let content = std::fs::read_to_string(&paths[0]).unwrap();
        assert!(content.contains("ffi-server - the ffi-server toolset"));
    }

    #[test]
    fn ffi_generates_python_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths =
            generate_client_artifacts(FfiClientArtifactKind::Python, generator_config(dir.path()))
                .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(
            paths[0].extension().and_then(|ext| ext.to_str()),
            Some("py")
        );
    }

    #[test]
    fn ffi_generates_typescript_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = generate_client_artifacts(
            FfiClientArtifactKind::TypeScript,
            generator_config(dir.path()),
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .any(|path| path.extension().and_then(|ext| ext.to_str()) == Some("ts")));
        assert!(paths.iter().any(|path| path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".d.ts"))));
    }

    #[test]
    fn ffi_parses_mcp_config() {
        let parsed = parse_mcp_config(
            r#"{
                "mcpServers": {
                    "my server": {
                        "command": "python3",
                        "args": ["server.py"],
                        "env": { "A": "B" }
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "my server");
        assert_eq!(parsed[0].cli_prefix, "my-server");
        assert_eq!(parsed[0].env, vec![("A".to_string(), "B".to_string())]);
    }
}
