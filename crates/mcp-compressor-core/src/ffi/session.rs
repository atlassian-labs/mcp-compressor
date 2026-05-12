use crate::proxy::{RunningToolProxy, ToolProxyServer};
use crate::server::{CompressedServer, CompressedServerConfig, ProxyTransformMode};
use crate::Error;

use super::dto::{
    FfiBackendConfig, FfiCompressedSessionConfig, FfiCompressedSessionInfo, FfiSdkServerConfig,
    FfiSdkServersConfig, FfiTool,
};

pub fn normalize_sdk_servers(servers: FfiSdkServersConfig) -> Result<Vec<FfiBackendConfig>, Error> {
    servers
        .into_iter()
        .map(|(name, config)| normalize_sdk_server(name, config))
        .collect()
}

fn normalize_sdk_server(
    name: String,
    config: FfiSdkServerConfig,
) -> Result<FfiBackendConfig, Error> {
    match config {
        FfiSdkServerConfig::CommandOrUrl(command_or_url) => Ok(FfiBackendConfig {
            name,
            command_or_url,
            args: Vec::new(),
            oauth_app_name: None,
        }),
        FfiSdkServerConfig::Structured {
            command,
            url,
            mut args,
            headers,
            oauth_app_name,
        } => {
            let command_or_url = url
                .or(command)
                .ok_or_else(|| Error::Config(format!("server {name} must define command or url")))?;
            if !headers.is_empty() {
                let mut header_args = Vec::new();
                for (key, value) in headers {
                    header_args.push("-H".to_string());
                    header_args.push(format!("{key}={value}"));
                }
                if !args.iter().any(|arg| arg == "--auth") {
                    header_args.push("--auth".to_string());
                    header_args.push("explicit-headers".to_string());
                }
                header_args.extend(args);
                args = header_args;
            }
            Ok(FfiBackendConfig {
                name,
                command_or_url,
                args,
                oauth_app_name,
            })
        }
    }
}

pub struct FfiCompressedSession {
    info: FfiCompressedSessionInfo,
    _proxy: RunningToolProxy,
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

    pub fn close(self) {}
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
        .map(FfiTool::from)
        .collect();
    let backend_tools = server.backend_tools().into_iter().map(FfiTool::from).collect();
    let backend_tools_by_server = server
        .backend_tools_by_server()
        .into_iter()
        .map(|(server_name, tool)| super::dto::FfiBackendTool {
            server_name,
            tool: FfiTool::from(tool),
        })
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
            backend_tools,
            backend_tools_by_server,
            just_bash_providers,
        },
        _proxy: proxy,
    })
}

pub async fn start_compressed_session(
    config: FfiCompressedSessionConfig,
    backends: Vec<FfiBackendConfig>,
) -> Result<FfiCompressedSession, Error> {
    start_compressed_session_with_backend_configs(
        config,
        backends.into_iter().map(Into::into).collect(),
    )
    .await
}

pub async fn start_compressed_session_with_backend_configs(
    config: FfiCompressedSessionConfig,
    backends: Vec<crate::server::BackendServerConfig>,
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
        backends,
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
