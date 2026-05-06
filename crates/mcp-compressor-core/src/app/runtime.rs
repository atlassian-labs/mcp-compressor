use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crate::client_gen::cli::CliGenerator;
use crate::client_gen::{ClientGenerator, GeneratorConfig};
use crate::proxy::ToolProxyServer;
use crate::server::registration::FrontendServer;
use crate::server::CompressedServer;

use super::options::{CliOptions, FrontendTransport};
use super::paths::cli_output_dir;

pub async fn run_compressed_server(
    cli: &CliOptions,
    server: CompressedServer,
) -> Result<(), String> {
    match cli.transport {
        FrontendTransport::Stdio => run_compressed_stdio(server).await,
        FrontendTransport::StreamableHttp => run_compressed_streamable_http(cli, server).await,
    }
}

async fn run_compressed_stdio(server: CompressedServer) -> Result<(), String> {
    rmcp::serve_server(FrontendServer::new(server), rmcp::transport::stdio())
        .await
        .map_err(|error| error.to_string())?
        .waiting()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn run_compressed_streamable_http(
    cli: &CliOptions,
    server: CompressedServer,
) -> Result<(), String> {
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
        .map_err(|error| error.to_string())?;
    let addr = listener.local_addr().map_err(|error| error.to_string())?;
    eprintln!("Streamable HTTP MCP server listening on http://{addr}/mcp");
    axum::serve(listener, router)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn run_just_bash_mode(cli: CliOptions, server: CompressedServer) -> Result<(), String> {
    let proxy = ToolProxyServer::start(server)
        .await
        .map_err(|error| error.to_string())?;
    let cli_name = cli
        .server_name
        .clone()
        .unwrap_or_else(|| "bash".to_string());

    println!("Just Bash ready");
    println!("Bridge URL: {}", proxy.bridge_url());
    println!("Use backend commands through the generated bridge. Full just-bash AST execution is not implemented in Rust yet.");
    println!("Session: {cli_name}");
    println!("Press Ctrl+C to stop.");

    wait_until_stopped().await;
    Ok(())
}

pub async fn run_cli_mode(cli: CliOptions, server: CompressedServer) -> Result<(), String> {
    let tools = server.single_backend_tools().map_err(|error| error.to_string())?;
    let proxy = ToolProxyServer::start(server)
        .await
        .map_err(|error| error.to_string())?;
    let (output_dir, on_path) = cli_output_dir().map_err(|error| error.to_string())?;
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
        .map_err(|error| error.to_string())?;
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

    wait_until_stopped().await;
    Ok(())
}

async fn wait_until_stopped() {
    if std::env::var_os("MCP_COMPRESSOR_EXIT_AFTER_READY").is_some() {
        return;
    }

    std::future::pending::<()>().await;
}
