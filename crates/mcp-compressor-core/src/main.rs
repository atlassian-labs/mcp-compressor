//! CLI entrypoint for the standalone Rust mcp-compressor core binary.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;

use clap::{ArgAction, Parser, ValueEnum};
use mcp_compressor_core::client_gen::cli::CliGenerator;
use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::proxy::ToolProxyServer;
use mcp_compressor_core::server::registration::FrontendServer;
use mcp_compressor_core::server::{
    BackendConfigSource, BackendServerConfig, CompressedServer, CompressedServerConfig,
    ProxyTransformMode,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
        Err(CliError::Runtime(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), CliError> {
    let cli = CliOptions::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    runtime.block_on(async move { run_async(cli).await })
}

async fn run_async(cli: CliOptions) -> Result<(), CliError> {
    let server = build_server(&cli).await?;

    match cli.transform_mode() {
        ProxyTransformMode::Cli => run_cli_mode(cli, server).await,
        ProxyTransformMode::CompressedTools => run_compressed_server(&cli, server).await,
        ProxyTransformMode::JustBash => run_just_bash_mode(cli, server).await,
    }
}

async fn build_server(cli: &CliOptions) -> Result<CompressedServer, CliError> {
    let config = CompressedServerConfig {
        level: cli.compression(),
        server_name: cli.server_name.clone(),
        include_tools: Vec::new(),
        exclude_tools: Vec::new(),
        toonify: false,
        transform_mode: cli.transform_mode(),
        config_source: BackendConfigSource::Command,
    };

    if let Some(config_path) = &cli.config_path {
        let json = std::fs::read_to_string(config_path)
            .map_err(|error| CliError::Runtime(error.to_string()))?;
        let mut config = config;
        let parsed = mcp_compressor_core::config::topology::MCPConfig::from_json(&json)
            .map_err(|error| CliError::Runtime(error.to_string()))?;
        config.config_source = if parsed.server_names().len() == 1 {
            BackendConfigSource::SingleServerJsonConfig
        } else {
            BackendConfigSource::MultiServerJsonConfig
        };
        CompressedServer::connect_mcp_config_json(config, &json)
            .await
            .map_err(|error| CliError::Runtime(error.to_string()))
    } else {
        if !cli.multi_server.is_empty() {
            return CompressedServer::connect_multi_stdio(
                config,
                cli.multi_server.iter().cloned().map(Into::into).collect(),
            )
            .await
            .map_err(|error| CliError::Runtime(error.to_string()));
        }
        let (command, args) = cli
            .command
            .split_first()
            .ok_or_else(|| CliError::Usage("backend command is required".to_string()))?;
        let backend_name = cli
            .server_name
            .clone()
            .unwrap_or_else(|| "server".to_string());
        CompressedServer::connect_stdio(
            config,
            BackendServerConfig::new(backend_name, command.clone(), args.to_vec()),
        )
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))
    }
}

async fn run_compressed_server(cli: &CliOptions, server: CompressedServer) -> Result<(), CliError> {
    match cli.transport {
        FrontendTransport::Stdio => run_compressed_stdio(server).await,
        FrontendTransport::StreamableHttp => run_compressed_streamable_http(cli, server).await,
    }
}

async fn run_compressed_stdio(server: CompressedServer) -> Result<(), CliError> {
    rmcp::serve_server(FrontendServer::new(server), rmcp::transport::stdio())
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?
        .waiting()
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    Ok(())
}

async fn run_compressed_streamable_http(
    cli: &CliOptions,
    server: CompressedServer,
) -> Result<(), CliError> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, tower::StreamableHttpService,
        StreamableHttpServerConfig,
    };

    let service = StreamableHttpService::new(
        {
            let server = Arc::new(server);
            move || Ok(FrontendServer::from_arc(server.clone()))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_sse_keep_alive(None),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, cli.port)))
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    eprintln!("Streamable HTTP MCP server listening on http://{addr}/mcp");
    axum::serve(listener, router)
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    Ok(())
}

async fn run_just_bash_mode(cli: CliOptions, server: CompressedServer) -> Result<(), CliError> {
    let proxy = ToolProxyServer::start(server)
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let cli_name = cli
        .server_name
        .clone()
        .unwrap_or_else(|| "bash".to_string());

    println!("Just Bash ready");
    println!("Bridge URL: {}", proxy.bridge_url());
    println!("Use backend commands through the generated bridge. Full just-bash AST execution is not implemented in Rust yet.");
    println!("Session: {cli_name}");
    println!("Press Ctrl+C to stop.");

    if std::env::var_os("MCP_COMPRESSOR_EXIT_AFTER_READY").is_some() {
        return Ok(());
    }

    std::future::pending::<()>().await;
    Ok(())
}

async fn run_cli_mode(cli: CliOptions, server: CompressedServer) -> Result<(), CliError> {
    let tools = server
        .single_backend_tools()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let proxy = ToolProxyServer::start(server)
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let (output_dir, on_path) = cli_output_dir(&cli)?;
    let cli_name = cli.server_name.clone().unwrap_or_else(|| "mcp".to_string());
    let config = GeneratorConfig {
        cli_name: cli_name.clone(),
        bridge_url: proxy.bridge_url().to_string(),
        token: proxy.token_value().to_string(),
        tools,
        session_pid: std::process::id(),
        output_dir,
    };
    let paths = CliGenerator
        .generate(&config)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    let script = paths
        .iter()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(cli_name.as_str()))
        .unwrap_or(&paths[0]);

    println!("CLI ready");
    println!("Generated CLI: {}", script.display());
    if on_path {
        println!("Invoke with: {cli_name} <subcommand> [args...]");
    } else {
        println!("Invoke with: {} <subcommand> [args...]", script.display());
        println!(
            "Note: {} is not on PATH; add it to PATH to run `{cli_name}` directly.",
            script
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .display()
        );
    }
    println!("Bridge URL: {}", proxy.bridge_url());
    println!("Press Ctrl+C to stop.");

    if std::env::var_os("MCP_COMPRESSOR_EXIT_AFTER_READY").is_some() {
        return Ok(());
    }

    std::future::pending::<()>().await;
    Ok(())
}

fn cli_output_dir(_cli: &CliOptions) -> Result<(PathBuf, bool), CliError> {
    if let Some(path) = std::env::var_os("MCP_COMPRESSOR_CLI_OUTPUT_DIR") {
        return Ok((PathBuf::from(path), true));
    }

    let path_dirs = path_dirs();
    for candidate in candidate_script_dirs() {
        let resolved = candidate.canonicalize().unwrap_or(candidate.clone());
        if resolved.is_dir() && path_dirs.iter().any(|path_dir| path_dir == &resolved) {
            return Ok((resolved, true));
        }
    }

    Ok((
        std::env::current_dir().map_err(|error| CliError::Runtime(error.to_string()))?,
        false,
    ))
}

fn candidate_script_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if cfg!(windows) {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WindowsApps"),
            );
        }
        if let Some(home) = &home {
            candidates.push(home.join(".local").join("bin"));
        }
    } else {
        if let Some(home) = &home {
            candidates.push(home.join(".local").join("bin"));
            candidates.push(home.join("bin"));
        }
        candidates.push(PathBuf::from("/usr/local/bin"));
        candidates.push(PathBuf::from("/opt/homebrew/bin"));
    }
    candidates
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|entry| entry.canonicalize().unwrap_or(entry))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Parser)]
#[command(
    name = "mcp-compressor-core",
    about = "Standalone Rust MCP compressor core binary",
    disable_help_subcommand = true
)]
struct CliOptions {
    /// Compression level: low, medium, high, or max.
    #[arg(long, value_enum, default_value = "medium")]
    compression: CompressionLevelArg,

    /// MCP config JSON file.
    #[arg(long = "config")]
    config_path: Option<PathBuf>,

    /// Frontend server name/prefix.
    #[arg(long)]
    server_name: Option<String>,

    /// Frontend transform mode.
    #[arg(long, value_enum, default_value = "compressed-tools")]
    transform_mode: TransformModeArg,

    /// Alias for --transform-mode cli.
    #[arg(long, action = ArgAction::SetTrue)]
    cli_mode: bool,

    /// Alias for --transform-mode just-bash.
    #[arg(long, action = ArgAction::SetTrue)]
    just_bash: bool,

    /// Multi-server backend spec: name=command [args...]. Repeat for each backend.
    #[arg(long = "multi-server", value_name = "NAME=COMMAND [ARGS...]", action = ArgAction::Append)]
    multi_server: Vec<MultiServerArg>,

    /// Frontend transport.
    #[arg(long, value_enum, default_value = "stdio")]
    transport: FrontendTransport,

    /// Port for streamable-http frontend; 0 chooses an available port.
    #[arg(long, default_value_t = 8000)]
    port: u16,

    /// Backend command, URL, and arguments. All backend server arguments belong after `--`.
    #[arg(value_name = "COMMAND", allow_hyphen_values = true, last = true)]
    command: Vec<String>,
}

impl CliOptions {
    fn compression(&self) -> CompressionLevel {
        self.compression.into()
    }

    fn transform_mode(&self) -> ProxyTransformMode {
        if self.just_bash {
            ProxyTransformMode::JustBash
        } else if self.cli_mode {
            ProxyTransformMode::Cli
        } else {
            self.transform_mode.into()
        }
    }
}

#[derive(Debug, Clone)]
struct MultiServerArg {
    name: String,
    command: String,
    args: Vec<String>,
}

impl FromStr for MultiServerArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split_whitespace();
        let spec = parts
            .next()
            .ok_or_else(|| "expected name=command".to_string())?;
        let (name, command) = spec
            .split_once('=')
            .filter(|(name, command)| !name.is_empty() && !command.is_empty())
            .ok_or_else(|| "expected name=command".to_string())?;
        Ok(Self {
            name: name.to_string(),
            command: command.to_string(),
            args: parts.map(ToString::to_string).collect(),
        })
    }
}

impl From<MultiServerArg> for BackendServerConfig {
    fn from(value: MultiServerArg) -> Self {
        BackendServerConfig::new(value.name, value.command, value.args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CompressionLevelArg {
    Low,
    Medium,
    High,
    Max,
}

impl std::fmt::Display for CompressionLevelArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        })
    }
}

impl From<CompressionLevelArg> for CompressionLevel {
    fn from(value: CompressionLevelArg) -> Self {
        match value {
            CompressionLevelArg::Low => CompressionLevel::Low,
            CompressionLevelArg::Medium => CompressionLevel::Medium,
            CompressionLevelArg::High => CompressionLevel::High,
            CompressionLevelArg::Max => CompressionLevel::Max,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TransformModeArg {
    CompressedTools,
    Cli,
    JustBash,
}

impl std::fmt::Display for TransformModeArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CompressedTools => "compressed-tools",
            Self::Cli => "cli",
            Self::JustBash => "just-bash",
        })
    }
}

impl From<TransformModeArg> for ProxyTransformMode {
    fn from(value: TransformModeArg) -> Self {
        match value {
            TransformModeArg::CompressedTools => ProxyTransformMode::CompressedTools,
            TransformModeArg::Cli => ProxyTransformMode::Cli,
            TransformModeArg::JustBash => ProxyTransformMode::JustBash,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FrontendTransport {
    Stdio,
    StreamableHttp,
}

impl std::fmt::Display for FrontendTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Stdio => "stdio",
            Self::StreamableHttp => "streamable-http",
        })
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Runtime(String),
}
