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
            let command_or_url = url.or(command).ok_or_else(|| {
                Error::Config(format!("server {name} must define command or url"))
            })?;
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

    pub async fn close(self) -> Result<(), Error> {
        let Self {
            server,
            _proxy,
            info: _,
        } = self;
        let proxy_result = match _proxy {
            Some(proxy) => proxy
                .shutdown()
                .await
                .map_err(|error| Error::Io(std::io::Error::other(error))),
            None => Ok(()),
        };
        // Deliberately not ownership-based: connection tasks of a draining
        // bridge may still hold a clone, and requiring exclusive ownership
        // would skip the release and leak the backend process trees.
        let backend_result = server.shutdown_shared().await;
        proxy_result.and(backend_result)
    }
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
    let backend_tools = server
        .backend_tools()
        .into_iter()
        .map(FfiTool::from)
        .collect();
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

#[cfg(test)]
mod close_lifecycle_tests {
    use super::*;
    use crate::ffi::dto::{FfiBackendConfig, FfiCompressedSessionConfig};

    fn fixture(name: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[tokio::test]
    async fn close_releases_backends_even_while_the_server_is_still_shared() {
        check_close(false).await;
    }

    #[tokio::test]
    async fn close_surfaces_listener_failure_after_releasing_backends() {
        check_close(true).await;
    }

    async fn check_close(fail_listener: bool) {
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let mut session = start_compressed_session(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
                bridge: true,
            },
            vec![FfiBackendConfig {
                name: "alpha".to_string(),
                command_or_url: python,
                args: vec![fixture("alpha_server.py")],
                oauth_app_name: None,
            }],
        )
        .await
        .unwrap();

        // Stands in for an HTTP bridge connection task that has not finished
        // draining yet, so the session is not the only owner of the server.
        let shared = Arc::clone(&session.server);

        if fail_listener {
            crate::proxy::server::close_lifecycle_tests::fail_listener(
                session._proxy.as_mut().unwrap(),
            )
            .await;
        }
        let closed = session.close().await;

        let invoked = dispatch_exec(
            &shared,
            "alpha_invoke_tool".to_string(),
            serde_json::json!({ "tool_name": "echo", "tool_input": { "message": "after close" } }),
        )
        .await;
        assert!(
            invoked.is_err(),
            "the backend must be released by close(), but it still answered: {invoked:?}"
        );
        if fail_listener {
            let error = closed.expect_err("close must surface the listener panic");
            assert!(
                error.to_string().contains("lifecycle listener panic"),
                "{error}"
            );
        } else {
            closed.expect("close must release the session");
        }
    }
}
