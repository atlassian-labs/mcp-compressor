//! CLI entrypoint for the standalone Rust mcp-compressor core binary.

use std::path::PathBuf;
use std::process::ExitCode;

use mcp_compressor_core::client_gen::cli::CliGenerator;
use mcp_compressor_core::client_gen::{ClientGenerator, GeneratorConfig};
use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::proxy::ToolProxyServer;
use mcp_compressor_core::server::{
    BackendConfigSource, BackendServerConfig, CompressedServer, CompressedServerConfig,
    ProxyTransformMode,
};

const HELP: &str = "mcp-compressor-core\n\nUSAGE:\n    mcp-compressor-core [OPTIONS] [-- <COMMAND>...]\n\nOPTIONS:\n    --help                      Print help\n    --compression <LEVEL>       low | medium | high | max\n    --config <PATH>             MCP config JSON file\n    --server-name <NAME>        Frontend server name/prefix\n    --transport <TYPE>          stdio | streamable-http\n    --transform-mode <MODE>     compressed-tools | cli | just-bash\n    --cli-mode                  Alias for --transform-mode cli\n    --just-bash                 Alias for --transform-mode just-bash\n";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}\n\n{HELP}");
            ExitCode::from(2)
        }
        Err(CliError::Runtime(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), CliError> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{HELP}");
        return Ok(());
    }

    let cli = CliOptions::parse(&args)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    runtime.block_on(async move { run_async(cli).await })
}

async fn run_async(cli: CliOptions) -> Result<(), CliError> {
    let server = build_server(&cli).await?;

    match cli.transform_mode {
        ProxyTransformMode::Cli => run_cli_mode(cli, server).await,
        ProxyTransformMode::CompressedTools => run_compressed_listing(server).await,
        ProxyTransformMode::JustBash => Err(CliError::Runtime(
            "--just-bash runtime is not implemented yet".to_string(),
        )),
    }
}

async fn build_server(cli: &CliOptions) -> Result<CompressedServer, CliError> {
    let config = CompressedServerConfig {
        level: cli.compression.clone(),
        server_name: cli.server_name.clone(),
        include_tools: Vec::new(),
        exclude_tools: Vec::new(),
        toonify: false,
        transform_mode: cli.transform_mode,
        config_source: if cli.config_path.is_some() {
            BackendConfigSource::SingleServerJsonConfig
        } else {
            BackendConfigSource::Command
        },
    };

    if let Some(config_path) = &cli.config_path {
        let json = std::fs::read_to_string(config_path)
            .map_err(|error| CliError::Runtime(error.to_string()))?;
        CompressedServer::connect_mcp_config_json(config, &json)
            .await
            .map_err(|error| CliError::Runtime(error.to_string()))
    } else {
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

async fn run_compressed_listing(server: CompressedServer) -> Result<(), CliError> {
    for tool in server
        .list_frontend_tools()
        .await
        .map_err(|error| CliError::Runtime(error.to_string()))?
    {
        println!("{}", tool.name);
    }
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
            script.parent().unwrap_or_else(|| std::path::Path::new(".")).display()
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
            candidates.push(PathBuf::from(local_app_data).join("Microsoft").join("WindowsApps"));
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

#[derive(Debug)]
struct CliOptions {
    compression: CompressionLevel,
    config_path: Option<PathBuf>,
    server_name: Option<String>,
    transform_mode: ProxyTransformMode,
    command: Vec<String>,
}

impl CliOptions {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut options = Self {
            compression: CompressionLevel::Medium,
            config_path: None,
            server_name: None,
            transform_mode: ProxyTransformMode::CompressedTools,
            command: Vec::new(),
        };
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--" => {
                    options.command = args[index + 1..].to_vec();
                    break;
                }
                "--compression" => {
                    let value = required_value(args, index, "--compression")?;
                    options.compression = value
                        .parse()
                        .map_err(|_| CliError::Usage(format!("unknown compression level: {value}")))?;
                    index += 2;
                }
                "--config" => {
                    options.config_path = Some(PathBuf::from(required_value(args, index, "--config")?));
                    index += 2;
                }
                "--server-name" => {
                    options.server_name = Some(required_value(args, index, "--server-name")?);
                    index += 2;
                }
                "--transport" => {
                    let value = required_value(args, index, "--transport")?;
                    if value != "stdio" {
                        return Err(CliError::Usage(format!(
                            "unsupported transport for CLI runtime: {value}"
                        )));
                    }
                    index += 2;
                }
                "--transform-mode" => {
                    options.transform_mode = parse_transform_mode(&required_value(
                        args,
                        index,
                        "--transform-mode",
                    )?)?;
                    index += 2;
                }
                "--cli-mode" => {
                    options.transform_mode = ProxyTransformMode::Cli;
                    index += 1;
                }
                "--just-bash" => {
                    options.transform_mode = ProxyTransformMode::JustBash;
                    index += 1;
                }
                option if option.starts_with('-') => {
                    return Err(CliError::Usage(format!("unknown option: {option}")));
                }
                _ => {
                    options.command = args[index..].to_vec();
                    break;
                }
            }
        }
        Ok(options)
    }
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<String, CliError> {
    args.get(index + 1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))
}

fn parse_transform_mode(value: &str) -> Result<ProxyTransformMode, CliError> {
    match value {
        "compressed-tools" => Ok(ProxyTransformMode::CompressedTools),
        "cli" => Ok(ProxyTransformMode::Cli),
        "just-bash" => Ok(ProxyTransformMode::JustBash),
        _ => Err(CliError::Usage(format!("unknown transform mode: {value}"))),
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Runtime(String),
}
