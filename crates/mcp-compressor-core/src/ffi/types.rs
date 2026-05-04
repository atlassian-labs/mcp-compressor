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
use crate::oauth::{
    clear_oauth_store, list_oauth_stores, oauth_store_root, remember_oauth_store,
    OAuthStoreIndexEntry,
};
use crate::proxy::ToolProxyServer;
use crate::server::{
    BackendServerConfig, CompressedServer, CompressedServerConfig, JustBashCommandSpec,
    JustBashProviderSpec, ProxyTransformMode,
};
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FfiOAuthStoreEntry {
    pub backend_name: String,
    pub backend_uri: String,
    pub store_dir: PathBuf,
}

pub fn oauth_store_path() -> PathBuf {
    oauth_store_root()
}

pub fn remember_oauth_backend(
    backend_uri: &str,
    backend_name: &str,
    store_dir: PathBuf,
) -> Result<(), Error> {
    remember_oauth_store(backend_uri, backend_name, &store_dir).map_err(Error::Io)
}

pub fn list_oauth_credentials() -> Result<Vec<FfiOAuthStoreEntry>, Error> {
    list_oauth_stores()
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .map_err(Error::Io)
}

pub fn clear_oauth_credentials(target: Option<&str>) -> Result<Vec<PathBuf>, Error> {
    clear_oauth_store(target).map_err(Error::Io)
}

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

pub struct FfiCompressedSession {
    info: FfiCompressedSessionInfo,
}

impl FfiCompressedSession {
    pub fn bridge_url(&self) -> &str {
        &self.info.bridge_url
    }

    pub fn token(&self) -> &str {
        &self.info.token
    }

    pub fn info(&self) -> FfiCompressedSessionInfo {
        self.info.clone()
    }
}

fn parse_ffi_transform_mode(value: Option<&str>) -> Result<ProxyTransformMode, Error> {
    match value.unwrap_or("compressed-tools") {
        "compressed-tools" | "compressed" | "normal" => Ok(ProxyTransformMode::CompressedTools),
        "cli" | "cli-mode" => Ok(ProxyTransformMode::Cli),
        "just-bash" | "just_bash" => Ok(ProxyTransformMode::JustBash),
        other => Err(Error::Config(format!("invalid transform mode: {other}"))),
    }
}

async fn compressed_session_from_server(
    server: CompressedServer,
) -> Result<FfiCompressedSession, Error> {
    let frontend_tools = server
        .list_frontend_tools()
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    let just_bash_providers = server
        .just_bash_provider_specs()
        .into_iter()
        .map(Into::into)
        .collect();
    let proxy = ToolProxyServer::start(server).await?;
    Ok(FfiCompressedSession {
        info: FfiCompressedSessionInfo {
            bridge_url: proxy.bridge_url().to_string(),
            token: proxy.token_value().to_string(),
            frontend_tools,
            just_bash_providers,
        },
    })
}

pub async fn start_compressed_session(
    config: FfiCompressedSessionConfig,
    backends: Vec<FfiBackendConfig>,
) -> Result<FfiCompressedSession, Error> {
    let server = CompressedServer::connect_multi_stdio(
        CompressedServerConfig {
            level: config.compression_level.parse()?,
            server_name: config.server_name,
            include_tools: config.include_tools,
            exclude_tools: config.exclude_tools,
            toonify: config.toonify,
            transform_mode: parse_ffi_transform_mode(config.transform_mode.as_deref())?,
            ..CompressedServerConfig::default()
        },
        backends.into_iter().map(Into::into).collect(),
    )
    .await?;
    compressed_session_from_server(server).await
}

pub async fn start_compressed_session_from_mcp_config(
    config: FfiCompressedSessionConfig,
    mcp_config_json: &str,
) -> Result<FfiCompressedSession, Error> {
    let server = CompressedServer::connect_mcp_config_json(
        CompressedServerConfig {
            level: config.compression_level.parse()?,
            server_name: config.server_name,
            include_tools: config.include_tools,
            exclude_tools: config.exclude_tools,
            toonify: config.toonify,
            transform_mode: parse_ffi_transform_mode(config.transform_mode.as_deref())?,
            ..CompressedServerConfig::default()
        },
        mcp_config_json,
    )
    .await?;
    compressed_session_from_server(server).await
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

impl From<OAuthStoreIndexEntry> for FfiOAuthStoreEntry {
    fn from(value: OAuthStoreIndexEntry) -> Self {
        Self {
            backend_name: value.name,
            backend_uri: value.uri,
            store_dir: value.store_dir.into(),
        }
    }
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
    fn ffi_lists_and_clears_oauth_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());

        let store_dir = oauth_store_path().join("example-store");
        std::fs::create_dir_all(&store_dir).unwrap();
        remember_oauth_backend("https://example.test/mcp", "example", store_dir.clone()).unwrap();

        let entries = list_oauth_credentials().unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry.backend_name == "example")
            .expect("remembered entry");
        assert_eq!(entry.backend_uri, "https://example.test/mcp");
        assert_eq!(entry.store_dir, store_dir);

        let cleared = clear_oauth_credentials(Some("example")).unwrap();
        assert!(cleared.iter().any(|path| path.ends_with("example-store")));
        assert!(!list_oauth_credentials()
            .unwrap()
            .iter()
            .any(|entry| entry.backend_name == "example"));

        if let Some(value) = previous {
            std::env::set_var("XDG_CONFIG_HOME", value);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
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

    async fn invoke_session(
        info: &FfiCompressedSessionInfo,
        tool: &str,
        tool_name: &str,
        tool_input: Value,
    ) -> String {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/exec", info.bridge_url))
            .bearer_auth(&info.token)
            .json(&serde_json::json!({
                "tool": tool,
                "input": {
                    "tool_name": tool_name,
                    "tool_input": tool_input
                }
            }))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.text().await.unwrap()
    }

    #[tokio::test]
    async fn ffi_starts_compressed_session_and_proxy() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
            },
            vec![FfiBackendConfig {
                name: "alpha".to_string(),
                command_or_url: std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                args: vec![fixture.to_string_lossy().into_owned()],
            }],
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info.bridge_url.starts_with("http://127.0.0.1:"));
        assert!(!info.token.is_empty());
        let invoke_tool_name = info
            .frontend_tools
            .iter()
            .find(|tool| tool.name.ends_with("invoke_tool"))
            .map(|tool| tool.name.clone())
            .expect("invoke wrapper tool");

        assert_eq!(
            invoke_session(
                &info,
                &invoke_tool_name,
                "echo",
                serde_json::json!({"message": "ffi"})
            )
            .await,
            "alpha:ffi"
        );
    }

    #[tokio::test]
    async fn ffi_starts_compressed_session_from_mcp_config_and_routes_multiple_servers() {
        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let config_json = serde_json::json!({
            "mcpServers": {
                "alpha": {
                    "command": python,
                    "args": [fixture_dir.join("alpha_server.py").to_string_lossy()]
                },
                "beta": {
                    "command": std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                    "args": [fixture_dir.join("beta_server.py").to_string_lossy()]
                }
            }
        })
        .to_string();
        let session = start_compressed_session_from_mcp_config(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: None,
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
            },
            &config_json,
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "alpha_invoke_tool"));
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "beta_invoke_tool"));
        assert_eq!(
            invoke_session(
                &info,
                "alpha_invoke_tool",
                "add",
                serde_json::json!({"a": 2, "b": 5})
            )
            .await,
            "7"
        );
        assert_eq!(
            invoke_session(
                &info,
                "beta_invoke_tool",
                "multiply",
                serde_json::json!({"a": 3, "b": 4})
            )
            .await,
            "12"
        );
    }

    #[tokio::test]
    async fn ffi_session_can_request_cli_transform_mode() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: Some("cli".to_string()),
            },
            vec![FfiBackendConfig {
                name: "alpha".to_string(),
                command_or_url: std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string()),
                args: vec![fixture.to_string_lossy().into_owned()],
            }],
        )
        .await
        .unwrap();
        let info = session.info();
        assert_eq!(info.frontend_tools.len(), 1);
        assert!(info.frontend_tools[0].name.ends_with("alpha_help"));
    }

    #[tokio::test]
    async fn ffi_session_can_request_just_bash_transform_mode() {
        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: None,
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: Some("just-bash".to_string()),
            },
            vec![
                FfiBackendConfig {
                    name: "alpha".to_string(),
                    command_or_url: python.clone(),
                    args: vec![fixture_dir
                        .join("alpha_server.py")
                        .to_string_lossy()
                        .into_owned()],
                },
                FfiBackendConfig {
                    name: "beta".to_string(),
                    command_or_url: python,
                    args: vec![fixture_dir
                        .join("beta_server.py")
                        .to_string_lossy()
                        .into_owned()],
                },
            ],
        )
        .await
        .unwrap();
        let info = session.info();
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "bash_tool"));
        assert!(info
            .frontend_tools
            .iter()
            .any(|tool| tool.name == "alpha_help"));
        assert_eq!(info.just_bash_providers.len(), 2);
        assert!(info
            .just_bash_providers
            .iter()
            .any(|provider| provider.provider_name == "alpha"));
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
