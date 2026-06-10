use std::sync::Arc;

use serde_json::Value;

use crate::proxy::{dispatch_exec, RunningToolProxy, ToolProxyServer};
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
    server: Arc<CompressedServer>,
    // Kept alive to keep the HTTP bridge running for out-of-process clients.
    // `None` for in-process (bridge-less) sessions.
    _proxy: Option<RunningToolProxy>,
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

    /// Frontend (compressed) tools exposed to callers, as DTOs.
    ///
    /// In-process equivalent of reading `info().frontend_tools`.
    pub fn list_frontend_tools(&self) -> Vec<FfiTool> {
        self.info.frontend_tools.clone()
    }

    /// Get the full backend schema for a tool via the compressed wrapper API,
    /// dispatched in-process (no HTTP bridge required).
    pub async fn get_tool_schema(
        &self,
        wrapper_tool_name: &str,
        backend_tool_name: &str,
    ) -> Result<String, Error> {
        self.server
            .get_tool_schema(wrapper_tool_name, backend_tool_name)
            .await
    }

    /// Invoke a frontend wrapper tool (or single-backend pass-through tool)
    /// in-process, reusing the session's live connection and OAuth.
    ///
    /// This shares [`dispatch_exec`] with the HTTP `/exec` bridge endpoint, so
    /// in-process and bridge invocations return identical payloads.
    pub async fn invoke(&self, tool: &str, input: Value) -> Result<String, Error> {
        dispatch_exec(&self.server, tool.to_string(), input).await
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
    bridge: bool,
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
    let (proxy, shared_server, bridge_url, token) = if bridge {
        let proxy = ToolProxyServer::start(server).await?;
        let bridge_url = proxy.bridge_url().to_string();
        let token = proxy.token_value().to_string();
        let shared_server = Arc::clone(proxy.server());
        (Some(proxy), shared_server, bridge_url, token)
    } else {
        let shared_server = ToolProxyServer::in_process(server);
        (None, shared_server, String::new(), String::new())
    };

    Ok(FfiCompressedSession {
        info: FfiCompressedSessionInfo {
            bridge_url,
            token,
            frontend_tools,
            backend_tools,
            backend_tools_by_server,
            just_bash_providers,
        },
        server: shared_server,
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
    let bridge = config.bridge;
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
    compressed_session_from_server(server, bridge).await
}

pub async fn start_compressed_session_from_mcp_config(
    config: FfiCompressedSessionConfig,
    mcp_config_json: &str,
) -> Result<FfiCompressedSession, Error> {
    let bridge = config.bridge;
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
    compressed_session_from_server(server, bridge).await
}
