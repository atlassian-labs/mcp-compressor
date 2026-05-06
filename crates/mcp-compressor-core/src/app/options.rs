use std::path::PathBuf;
use std::str::FromStr;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

use crate::compression::CompressionLevel;
use crate::server::{BackendServerConfig, ProxyTransformMode};

#[derive(Debug, Parser)]
#[command(
    name = "mcp-compressor-core",
    about = "Standalone Rust MCP compressor core binary",
    disable_help_subcommand = true
)]
pub struct CliOptions {
    #[command(subcommand)]
    pub command_kind: Option<CliCommand>,

    /// Compression level: low, medium, high, or max.
    #[arg(long, value_enum, default_value = "medium")]
    compression: CompressionLevelArg,

    /// MCP config JSON file.
    #[arg(long = "config")]
    pub config_path: Option<PathBuf>,

    /// Frontend server name/prefix.
    #[arg(long)]
    pub server_name: Option<String>,

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
    pub multi_server: Vec<MultiServerArg>,

    /// Frontend transport.
    #[arg(long, value_enum, default_value = "stdio")]
    pub transport: FrontendTransport,

    /// Port for streamable-http frontend; 0 chooses an available port.
    #[arg(long, default_value_t = 8000)]
    pub port: u16,

    /// Backend command, URL, and arguments. All backend server arguments belong after `--`.
    #[arg(value_name = "COMMAND", allow_hyphen_values = true, last = true)]
    pub command: Vec<String>,
}

impl CliOptions {
    pub fn compression(&self) -> CompressionLevel {
        self.compression.into()
    }

    pub fn transform_mode(&self) -> ProxyTransformMode {
        if self.just_bash {
            ProxyTransformMode::JustBash
        } else if self.cli_mode {
            ProxyTransformMode::Cli
        } else {
            self.transform_mode.into()
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    /// Clear stored OAuth credentials.
    ClearOauth {
        /// Backend server name or URL to clear. If omitted, all Rust OAuth state is removed.
        target: Option<String>,
    },
}

impl CliCommand {
    pub fn clear_oauth_target(&self) -> Option<&str> {
        match self {
            Self::ClearOauth { target } => target.as_deref(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MultiServerArg {
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
pub enum FrontendTransport {
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
