use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use serde_json::{json, Value};

use crate::client_gen::cli::CliGenerator;
use crate::client_gen::generator::{ClientGenerator, GeneratorConfig};
use crate::client_gen::python::PythonGenerator;
use crate::client_gen::typescript::TypeScriptGenerator;
use crate::compression::engine::Tool;
use crate::compression::CompressionLevel;
use crate::ffi::{normalize_sdk_servers, FfiSdkServerConfig, FfiSdkServersConfig};
use crate::proxy::{RunningToolProxy, ToolProxyServer};
use crate::server::{BackendAuthMode, BackendServerConfig};
use crate::server::{CompressedServer, CompressedServerConfig, ProxyTransformMode};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressorMode {
    CompressedTools,
    Cli,
    JustBash,
}

impl From<CompressorMode> for ProxyTransformMode {
    fn from(value: CompressorMode) -> Self {
        match value {
            CompressorMode::CompressedTools => Self::CompressedTools,
            CompressorMode::Cli => Self::Cli,
            CompressorMode::JustBash => Self::JustBash,
        }
    }
}

type HeaderProvider = Arc<dyn Fn() -> Result<BTreeMap<String, String>, Error> + Send + Sync>;

#[derive(Clone)]
pub struct ServerConfig {
    inner: FfiSdkServerConfig,
    auth_provider: Option<HeaderProvider>,
    oauth_app_name: Option<String>,
}

impl ServerConfig {
    pub fn command(command: impl Into<String>) -> Self {
        Self {
            inner: FfiSdkServerConfig::Structured {
                command: Some(command.into()),
                url: None,
                args: Vec::new(),
                headers: BTreeMap::new(),
                oauth_app_name: None,
            },
            auth_provider: None,
            oauth_app_name: None,
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self {
            inner: FfiSdkServerConfig::Structured {
                command: None,
                url: Some(url.into()),
                args: Vec::new(),
                headers: BTreeMap::new(),
                oauth_app_name: None,
            },
            auth_provider: None,
            oauth_app_name: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        if let FfiSdkServerConfig::Structured { args, .. } = &mut self.inner {
            args.push(arg.into());
        }
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        if let FfiSdkServerConfig::Structured { args: stored, .. } = &mut self.inner {
            stored.extend(args.into_iter().map(Into::into));
        }
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        if let FfiSdkServerConfig::Structured { headers, .. } = &mut self.inner {
            headers.insert(name.into(), value.into());
        }
        self
    }

    pub fn auth_provider(
        mut self,
        provider: impl Fn() -> Result<BTreeMap<String, String>, Error> + Send + Sync + 'static,
    ) -> Self {
        self.auth_provider = Some(Arc::new(provider));
        self
    }

    pub fn oauth_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.oauth_app_name = Some(app_name.into());
        self
    }

    fn materialize(mut self) -> (FfiSdkServerConfig, Option<HeaderProvider>) {
        if let (FfiSdkServerConfig::Structured { oauth_app_name, .. }, Some(app_name)) =
            (&mut self.inner, self.oauth_app_name.take())
        {
            *oauth_app_name = Some(app_name);
        }
        (self.inner, self.auth_provider.take())
    }
}

#[derive(Clone)]
pub struct CompressorClientBuilder {
    servers: BTreeMap<String, ServerConfig>,
    compression_level: CompressionLevel,
    server_name: Option<String>,
    include_tools: Vec<String>,
    exclude_tools: Vec<String>,
    toonify: bool,
    mode: CompressorMode,
}

impl Default for CompressorClientBuilder {
    fn default() -> Self {
        Self {
            servers: BTreeMap::new(),
            compression_level: CompressionLevel::Max,
            server_name: None,
            include_tools: Vec::new(),
            exclude_tools: Vec::new(),
            toonify: false,
            mode: CompressorMode::CompressedTools,
        }
    }
}

impl CompressorClientBuilder {
    pub fn server(mut self, name: impl Into<String>, config: ServerConfig) -> Self {
        self.servers.insert(name.into(), config);
        self
    }

    pub fn compression_level(mut self, level: CompressionLevel) -> Self {
        self.compression_level = level;
        self
    }

    pub fn server_name(mut self, server_name: impl Into<String>) -> Self {
        self.server_name = Some(server_name.into());
        self
    }

    pub fn include_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.include_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    pub fn exclude_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    pub fn toonify(mut self, enabled: bool) -> Self {
        self.toonify = enabled;
        self
    }

    pub fn mode(mut self, mode: CompressorMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn build(self) -> CompressorClient {
        CompressorClient { builder: self }
    }
}

#[derive(Clone)]
pub struct CompressorClient {
    builder: CompressorClientBuilder,
}

impl CompressorClient {
    pub fn builder() -> CompressorClientBuilder {
        CompressorClientBuilder::default()
    }

    pub async fn connect(&self) -> Result<CompressorProxy, Error> {
        let materialized = self
            .builder
            .servers
            .clone()
            .into_iter()
            .map(|(name, config)| {
                let (config, provider) = config.materialize();
                (name, config, provider)
            })
            .collect::<Vec<_>>();
        let providers = materialized
            .iter()
            .filter_map(|(name, _, provider)| {
                provider.clone().map(|provider| (name.clone(), provider))
            })
            .collect::<BTreeMap<_, _>>();
        let ffi_configs = materialized
            .into_iter()
            .map(|(name, config, _)| (name, config))
            .collect::<Vec<_>>();
        let backends = normalize_sdk_servers(FfiSdkServersConfig::from_iter(ffi_configs))?;
        let backends = backends
            .into_iter()
            .map(|backend| {
                let name = backend.name.clone();
                let mut backend = BackendServerConfig::from(backend);
                if let Some(provider) = providers.get(&name) {
                    backend = backend
                        .with_header_provider(Arc::clone(provider))
                        .with_auth_mode(BackendAuthMode::ExplicitHeaders);
                }
                backend
            })
            .collect::<Vec<_>>();
        let server = CompressedServer::connect_multi_stdio(
            CompressedServerConfig {
                level: self.builder.compression_level.clone(),
                server_name: self.builder.server_name.clone(),
                include_tools: self.builder.include_tools.clone(),
                exclude_tools: self.builder.exclude_tools.clone(),
                toonify: self.builder.toonify,
                transform_mode: self.builder.mode.into(),
                ..CompressedServerConfig::default()
            },
            backends,
        )
        .await?;
        CompressorProxy::start(server).await
    }
}

pub struct CompressorProxy {
    default_server: Option<String>,
    frontend_tools: Vec<Tool>,
    backend_tools: Vec<Tool>,
    backend_tools_by_server: Vec<(String, Tool)>,
    just_bash_providers: Vec<crate::server::JustBashProviderSpec>,
    proxy: RunningToolProxy,
}

impl CompressorProxy {
    async fn start(server: CompressedServer) -> Result<Self, Error> {
        let default_server = server.default_server_name().map(str::to_string);
        let frontend_tools = server.list_frontend_tools().await?;
        let backend_tools = server.backend_tools();
        let backend_tools_by_server = server.backend_tools_by_server();
        let just_bash_providers = server.just_bash_provider_specs();
        let proxy = ToolProxyServer::start(server).await?;
        Ok(Self {
            default_server,
            frontend_tools,
            backend_tools,
            backend_tools_by_server,
            just_bash_providers,
            proxy,
        })
    }

    pub fn bridge_url(&self) -> &str {
        self.proxy.bridge_url()
    }

    pub fn token(&self) -> &str {
        self.proxy.token_value()
    }

    pub fn tools(&self) -> &[Tool] {
        &self.frontend_tools
    }

    pub fn backend_tools(&self) -> &[Tool] {
        &self.backend_tools
    }

    pub fn just_bash_providers(&self) -> &[crate::server::JustBashProviderSpec] {
        &self.just_bash_providers
    }

    pub fn schema(&self, tool_name: &str) -> Result<Value, Error> {
        self.schema_on(self.default_server.as_deref(), tool_name)
    }

    pub fn schema_on(&self, server: Option<&str>, tool_name: &str) -> Result<Value, Error> {
        let matches = self
            .backend_tools_by_server
            .iter()
            .filter(|(server_name, tool)| {
                tool.name == tool_name && server.map(|server| server == server_name).unwrap_or(true)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [(_, tool)] => Ok(tool.input_schema.clone()),
            [] => Err(Error::ToolNotFound(tool_name.to_string())),
            _ => Err(Error::Config(
                "Multiple backend tools matched; specify a server".to_string(),
            )),
        }
    }

    pub async fn invoke(&self, tool_name: &str, input: Value) -> Result<String, Error> {
        self.invoke_on(self.default_server.as_deref(), tool_name, input)
            .await
    }

    pub async fn invoke_on(
        &self,
        server: Option<&str>,
        tool_name: &str,
        input: Value,
    ) -> Result<String, Error> {
        let wrapper = self.invoke_wrapper(server)?;
        self.invoke_wrapper_tool(
            &wrapper,
            json!({
                "tool_name": tool_name,
                "tool_input": input,
            }),
        )
        .await
    }

    async fn invoke_wrapper_tool(&self, wrapper: &str, input: Value) -> Result<String, Error> {
        let client = reqwest::Client::new();
        let response = client
            .post(self.proxy.exec_url())
            .header("Authorization", format!("Bearer {}", self.token()))
            .json(&json!({
                "tool": wrapper,
                "input": input
            }))
            .send()
            .await
            .map_err(|error| Error::Config(format!("proxy request failed: {error}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| Error::Config(format!("proxy response failed: {error}")))?;
        if status.is_success() {
            Ok(text)
        } else {
            Err(Error::Config(format!(
                "proxy request failed with {status}: {text}"
            )))
        }
    }

    pub fn executable_tools(&self) -> BTreeMap<String, Box<dyn ExecutableTool + '_>> {
        self.frontend_tools
            .iter()
            .map(|tool| {
                (
                    tool.name.clone(),
                    Box::new(ProxyExecutableTool { proxy: self, tool: tool.clone() })
                        as Box<dyn ExecutableTool>,
                )
            })
            .collect()
    }

    pub fn write_cli_client(
        &self,
        output_dir: impl AsRef<Path>,
        name: Option<&str>,
    ) -> Result<GeneratedClient, Error> {
        self.write_client(GeneratedClientKind::Cli, output_dir, name)
    }

    pub fn write_code_client(
        &self,
        language: CodeLanguage,
        output_dir: impl AsRef<Path>,
        name: Option<&str>,
    ) -> Result<GeneratedClient, Error> {
        let kind = match language {
            CodeLanguage::Python => GeneratedClientKind::Python,
            CodeLanguage::TypeScript => GeneratedClientKind::TypeScript,
        };
        self.write_client(kind, output_dir, name.into())
    }

    pub fn write_client(
        &self,
        kind: GeneratedClientKind,
        output_dir: impl AsRef<Path>,
        name: Option<&str>,
    ) -> Result<GeneratedClient, Error> {
        let generator_config = GeneratorConfig {
            cli_name: name
                .or(self.default_server.as_deref())
                .unwrap_or("mcp")
                .to_string(),
            bridge_url: self.bridge_url().to_string(),
            token: self.token().to_string(),
            tools: self.backend_tools.clone(),
            session_pid: 0,
            output_dir: output_dir.as_ref().to_path_buf(),
        };
        let files = match kind {
            GeneratedClientKind::Cli => CliGenerator.generate(&generator_config),
            GeneratedClientKind::Python => PythonGenerator.generate(&generator_config),
            GeneratedClientKind::TypeScript => TypeScriptGenerator.generate(&generator_config),
        }?;
        let environment = kind.environment(&generator_config);
        Ok(GeneratedClient {
            kind,
            output_dir: generator_config.output_dir,
            files,
            environment,
        })
    }

    fn invoke_wrapper(&self, server: Option<&str>) -> Result<String, Error> {
        let suffix = "_invoke_tool";
        let matches = self
            .frontend_tools
            .iter()
            .filter(|tool| tool.name.ends_with(suffix))
            .filter(|tool| {
                server
                    .map(|name| tool.name == format!("{name}{suffix}"))
                    .unwrap_or(true)
            })
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [name] => Ok(name.clone()),
            [] => Err(Error::Config(format!(
                "No compressed invoke wrapper found for server {}",
                server.unwrap_or("<default>")
            ))),
            _ => Err(Error::Config(
                "Multiple compressed invoke wrappers found; specify a server".to_string(),
            )),
        }
    }
}

#[async_trait]
pub trait ExecutableTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn input_schema(&self) -> &Value;
    async fn execute(&self, input: Value) -> Result<String, Error>;
}

struct ProxyExecutableTool<'a> {
    proxy: &'a CompressorProxy,
    tool: Tool,
}

#[async_trait]
impl ExecutableTool for ProxyExecutableTool<'_> {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> Option<&str> {
        self.tool.description.as_deref()
    }

    fn input_schema(&self) -> &Value {
        &self.tool.input_schema
    }

    async fn execute(&self, input: Value) -> Result<String, Error> {
        self.proxy.invoke_wrapper_tool(&self.tool.name, input).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLanguage {
    Python,
    TypeScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedClient {
    pub kind: GeneratedClientKind,
    pub output_dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub environment: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedClientKind {
    Cli,
    Python,
    TypeScript,
}

impl GeneratedClientKind {
    fn environment(self, config: &GeneratorConfig) -> HashMap<String, String> {
        match self {
            GeneratedClientKind::Python => HashMap::from([(
                "PYTHONPATH".to_string(),
                config.output_dir.to_string_lossy().to_string(),
            )]),
            GeneratedClientKind::Cli | GeneratedClientKind::TypeScript => HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fixture_path(name: &str) -> String {
        format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn python_command() -> String {
        std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
    }

    #[test]
    fn server_config_oauth_app_name_is_preserved_for_transport_layer() {
        let config = ServerConfig::url("https://example.test/mcp")
            .oauth_app_name("Rovo Dev")
            .materialize()
            .0;

        match config {
            FfiSdkServerConfig::Structured { oauth_app_name, .. } => {
                assert_eq!(oauth_app_name.as_deref(), Some("Rovo Dev"));
            }
            FfiSdkServerConfig::CommandOrUrl(_) => panic!("expected structured config"),
        }
    }

    #[test]
    fn server_config_auth_provider_is_preserved_for_transport_layer() {
        let (config, provider) = ServerConfig::url("https://example.test/mcp")
            .header("X-Static", "yes")
            .auth_provider(|| {
                Ok(BTreeMap::from([(
                    "Authorization".to_string(),
                    "Bearer dynamic".to_string(),
                )]))
            })
            .materialize();

        let backends = normalize_sdk_servers(FfiSdkServersConfig::from_iter([(
            "remote".to_string(),
            config,
        )]))
        .unwrap();

        assert_eq!(backends[0].command_or_url, "https://example.test/mcp");
        assert_eq!(
            backends[0].args,
            ["-H", "X-Static=yes", "--auth", "explicit-headers"]
        );
        assert!(provider.is_some());
    }

    #[tokio::test]
    async fn compressor_client_invokes_single_server_without_compressor_subprocess() {
        let client = CompressorClient::builder()
            .server(
                "alpha",
                ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
            )
            .compression_level(CompressionLevel::Max)
            .build();
        let proxy = client.connect().await.unwrap();
        assert!(proxy
            .tools()
            .iter()
            .any(|tool| tool.name == "alpha_invoke_tool"));
        let result = proxy
            .invoke("echo", json!({ "message": "rust-sdk" }))
            .await
            .unwrap();
        assert_eq!(result, "alpha:rust-sdk");

        let executable = proxy.executable_tools();
        let invoke = executable.get("alpha_invoke_tool").unwrap();
        let executable_result = invoke
            .execute(json!({
                "tool_name": "echo",
                "tool_input": { "message": "executable-rust" }
            }))
            .await
            .unwrap();
        assert_eq!(executable_result, "alpha:executable-rust");
    }

    #[tokio::test]
    async fn compressor_client_routes_multiple_servers() {
        let client = CompressorClient::builder()
            .server(
                "alpha",
                ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
            )
            .server(
                "beta",
                ServerConfig::command(python_command()).arg(fixture_path("beta_server.py")),
            )
            .compression_level(CompressionLevel::Max)
            .build();
        let proxy = client.connect().await.unwrap();
        let alpha = proxy
            .invoke_on(Some("alpha"), "add", json!({ "a": 2, "b": 3 }))
            .await
            .unwrap();
        let beta = proxy
            .invoke_on(Some("beta"), "multiply", json!({ "a": 4, "b": 5 }))
            .await
            .unwrap();
        assert_eq!(alpha, "5");
        assert_eq!(beta, "20");
    }

    #[tokio::test]
    async fn compressor_client_writes_generated_clients() {
        let client = CompressorClient::builder()
            .server(
                "alpha",
                ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
            )
            .compression_level(CompressionLevel::Max)
            .build();
        let proxy = client.connect().await.unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let generated = proxy
            .write_code_client(CodeLanguage::Python, tempdir.path(), Some("alpha"))
            .unwrap();
        assert_eq!(generated.kind, GeneratedClientKind::Python);
        assert!(generated.files.iter().any(|path| path.ends_with("alpha.py")));
        assert_eq!(
            generated.environment.get("PYTHONPATH"),
            Some(&tempdir.path().to_string_lossy().to_string())
        );

        let cli = proxy.write_cli_client(tempdir.path(), Some("alpha")).unwrap();
        assert_eq!(cli.kind, GeneratedClientKind::Cli);
    }

    #[tokio::test]
    async fn compressor_client_exposes_cli_and_just_bash_modes() {
        let cli = CompressorClient::builder()
            .server(
                "alpha",
                ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
            )
            .mode(CompressorMode::Cli)
            .build()
            .connect()
            .await
            .unwrap();
        assert!(cli.tools().iter().any(|tool| tool.name == "alpha_help"));

        let bash = CompressorClient::builder()
            .server(
                "alpha",
                ServerConfig::command(python_command()).arg(fixture_path("alpha_server.py")),
            )
            .mode(CompressorMode::JustBash)
            .build()
            .connect()
            .await
            .unwrap();
        assert!(bash.tools().iter().any(|tool| tool.name == "bash_tool"));
        assert!(bash.tools().iter().any(|tool| tool.name == "alpha_help"));
        let provider = bash
            .just_bash_providers()
            .iter()
            .find(|provider| provider.provider_name == "alpha")
            .unwrap();
        assert_eq!(provider.help_tool_name, "alpha_help");
        assert!(provider.tools.iter().any(|command| {
            command.command_name == "echo"
                && command.backend_tool_name == "echo"
                && command.invoke_tool_name == "alpha_invoke_tool"
        }));
    }
}
