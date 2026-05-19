use std::process::ExitCode;

use clap::{error::ErrorKind, Parser};

use crate::app::options::{CliCommand, CliOptions, LlmCommand, MultiServerArg};
use crate::app::runtime::{run_cli_mode, run_compressed_server, run_just_bash_mode};
use crate::oauth::clear_oauth_store;
use crate::server::{
    BackendConfigSource, BackendServerConfig, CompressedServer, CompressedServerConfig,
    ProxyTransformMode,
};

pub fn main_exit_code() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Display(message)) => {
            print!("{message}");
            ExitCode::SUCCESS
        }
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

pub fn run() -> Result<(), CliError> {
    run_from(std::env::args())
}

pub fn run_from<I, T>(args: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = parse_cli(args)?;
    run_cli(cli)
}

pub fn parse_cli<I, T>(args: I) -> Result<CliOptions, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    CliOptions::try_parse_from(args).map_err(|error| {
        if matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        ) {
            CliError::Display(error.to_string())
        } else {
            CliError::Usage(error.to_string())
        }
    })
}

pub fn run_cli(cli: CliOptions) -> Result<(), CliError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    runtime.block_on(async move { run_async(cli).await })
}

async fn run_async(cli: CliOptions) -> Result<(), CliError> {
    cli.validate().map_err(CliError::Usage)?;
    if let Some(command) = &cli.command_kind {
        return run_command(command).await;
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
        let parsed = crate::config::topology::MCPConfig::from_json(&json)
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
                cli.multi_server
                    .iter()
                    .cloned()
                    .map(MultiServerArg::into_backend)
                    .collect(),
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

async fn run_command(command: &CliCommand) -> Result<(), CliError> {
    if let Some(llm_command) = command.llm_command() {
        return run_llm_command(llm_command).await;
    }
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

async fn run_llm_command(command: &LlmCommand) -> Result<(), CliError> {
    match command {
        LlmCommand::Status(options) => {
            let config = options.config(false).map_err(CliError::Usage)?;
            let status = crate::llm_assist::install_status(&config)
                .map_err(|error| CliError::Runtime(error.to_string()))?;
            println!(
                "llama-server: {}",
                if status.llama_server_ready {
                    "ready"
                } else {
                    "missing"
                }
            );
            if let Some(path) = status.llama_server_path {
                println!("llama-server path: {}", path.display());
            }
            println!("model: {}", status.model_ref);
            println!(
                "model status: {}",
                if status.model_ready {
                    "ready"
                } else {
                    "missing"
                }
            );
            println!("model path: {}", status.model_path.display());
        }
        LlmCommand::Pull(options) => {
            let config = options.config(true).map_err(CliError::Usage)?;
            let prepared = crate::llm_assist::pull_llm_assets(config)
                .await
                .map_err(|error| CliError::Runtime(error.to_string()))?;
            println!(
                "llama-server ready: {}",
                prepared.llama_server_path.display()
            );
            println!("model ready: {}", prepared.model_path.display());
        }
        LlmCommand::Remove(options) => {
            crate::llm_assist::remove_managed_llm_assets(options.cache_dir())
                .map_err(|error| CliError::Runtime(error.to_string()))?;
            println!("Removed managed LLM assets from the mcp-compressor cache.");
        }
        LlmCommand::Test(options) => {
            let config = options.config(true).map_err(CliError::Usage)?;
            let assistant = crate::llm_assist::LlmAssistant::from_config(config);
            assistant.start_background_preparation();
            let response = assistant
                .complete("You are a concise local utility model.", options.prompt())
                .await
                .map_err(|error| CliError::Runtime(error.to_string()))?;
            println!("{response}");
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum CliError {
    Display(String),
    Usage(String),
    Runtime(String),
}
