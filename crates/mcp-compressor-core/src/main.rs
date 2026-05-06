//! CLI entrypoint for the standalone Rust mcp-compressor core binary.

use std::process::ExitCode;

use clap::Parser;
use mcp_compressor_core::app::options::{CliCommand, CliOptions};
use mcp_compressor_core::app::runtime::{
    run_cli_mode, run_compressed_server, run_just_bash_mode,
};
use mcp_compressor_core::oauth::clear_oauth_store;
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
    .map_err(CliError::Runtime)
}

async fn build_server(cli: &CliOptions) -> Result<CompressedServer, CliError> {
    let config = CompressedServerConfig {
        level: cli.compression(),
        server_name: cli.server_name.clone(),
        include_tools: cli.include_tools.clone(),
        exclude_tools: cli.exclude_tools.clone(),
        toonify: cli.toonify,
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
