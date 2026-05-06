//! CLI entrypoint for the standalone Rust mcp-compressor core binary.

use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use mcp_compressor_core::app::options::{CliCommand, CliOptions, FrontendTransport};
use mcp_compressor_core::app::paths::cli_output_dir;
use mcp_compressor_core::client_gen::cli::CliGenerator;
use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
use mcp_compressor_core::oauth::clear_oauth_store;
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
    if let Some(command) = &cli.command_kind {
        return run_command(command);
    }
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
    let (output_dir, on_path) = cli_output_dir()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
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

fn run_command(command: &CliCommand) -> Result<(), CliError> {
    let removed = clear_oauth_store(command.clear_oauth_target())
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    if removed.is_empty() {
        println!("No stored OAuth credentials found.");
    } else {
        println!(
            "Removed {} OAuth store entr{}.",
            removed.len(),
            if removed.len() == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Runtime(String),
}
