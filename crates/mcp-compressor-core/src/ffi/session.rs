use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::Error;
use crate::proxy::{BeforeExecHook, RunningToolProxy, ToolProxyServer, dispatch_exec};
use crate::server::{CompressedServer, CompressedServerConfig, ProxyTransformMode};

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
    // Applied to in-process invocations. A bridge runs the same hook for its
    // own HTTP requests, but `invoke()` never goes through the bridge, so
    // without this it would use credentials the caller already considers stale.
    before_exec: Option<BeforeExecHook>,
    before_exec_lock: Arc<Mutex<()>>,
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
        if let Some(before_exec) = &self.before_exec {
            let _refresh = self.before_exec_lock.lock().await;
            before_exec().await?;
        }
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
    before_exec: Option<BeforeExecHook>,
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
    let before_exec_lock = Arc::new(Mutex::new(()));
    let (proxy, shared_server, bridge_url, token, in_process_before_exec) = if bridge {
        // The hook has to stay on the session as well. A bridged session still
        // dispatches `invoke()` in process, so it never reaches the bridge that
        // would otherwise run the refresh.
        let session_hook = before_exec.clone();
        let proxy = match before_exec {
            Some(hook) => {
                ToolProxyServer::start_with_before_exec_lock(
                    server,
                    hook,
                    Arc::clone(&before_exec_lock),
                )
                .await?
            }
            None => ToolProxyServer::start(server).await?,
        };
        let bridge_url = proxy.bridge_url().to_string();
        let token = proxy.token_value().to_string();
        let shared_server = Arc::clone(proxy.server());
        (Some(proxy), shared_server, bridge_url, token, session_hook)
    } else {
        let shared_server = ToolProxyServer::in_process(server);
        (
            None,
            shared_server,
            String::new(),
            String::new(),
            before_exec,
        )
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
        before_exec: in_process_before_exec,
        before_exec_lock,
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
    start_compressed_session_with_backend_configs_and_before_exec(config, backends, None).await
}

pub async fn start_compressed_session_with_backend_configs_and_before_exec(
    config: FfiCompressedSessionConfig,
    backends: Vec<crate::server::BackendServerConfig>,
    before_exec: Option<BeforeExecHook>,
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
    compressed_session_from_server(server, bridge, before_exec).await
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
    compressed_session_from_server(server, bridge, None).await
}

#[cfg(test)]
mod before_exec_tests {
    use super::*;
    use crate::ffi::dto::FfiCompressedSessionConfig;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Invoke one tool through a session built with the given bridge setting and
    /// report how many times the auth-refresh hook ran.
    async fn refreshes_during_one_invocation(bridge: bool) -> usize {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let refreshes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&refreshes);
        let hook: BeforeExecHook = Arc::new(move || {
            let counter = Arc::clone(&counter);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });

        let session = start_compressed_session_with_backend_configs_and_before_exec(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
                bridge,
            },
            vec![crate::server::BackendServerConfig::new(
                "alpha",
                python,
                [fixture.to_string_lossy().into_owned()],
            )],
            Some(hook),
        )
        .await
        .unwrap();

        let output = session
            .invoke(
                "alpha_alpha_invoke_tool",
                serde_json::json!({ "tool_name": "echo", "tool_input": { "message": "hi" } }),
            )
            .await
            .unwrap();

        assert!(output.contains("alpha:hi"));
        refreshes.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn in_process_invocations_run_the_auth_refresh_hook() {
        assert_eq!(
            refreshes_during_one_invocation(false).await,
            1,
            "a bridge-less session must still refresh auth before invoking, or generated \
             in-process calls reuse stale credentials"
        );
    }

    /// A bridge does not make the session hook redundant.
    ///
    /// `invoke()` dispatches in process whether or not a bridge is running, so
    /// a session that hands its hook to the bridge and keeps none for itself
    /// refreshes for HTTP callers and silently skips the refresh for SDK
    /// callers on the very same session.
    #[tokio::test]
    async fn bridged_sessions_still_run_the_auth_refresh_hook_in_process() {
        assert_eq!(
            refreshes_during_one_invocation(true).await,
            1,
            "a bridged session must refresh auth for its own in-process invocations, \
             not only for requests that arrive over the bridge"
        );
    }

    #[tokio::test]
    async fn concurrent_in_process_invocations_serialize_auth_refresh() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let hook: BeforeExecHook = {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            Arc::new(move || {
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                Box::pin(async move {
                    let concurrent = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(concurrent, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        };
        let session = start_compressed_session_with_backend_configs_and_before_exec(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
                bridge: false,
            },
            vec![crate::server::BackendServerConfig::new(
                "alpha",
                python,
                [fixture.to_string_lossy().into_owned()],
            )],
            Some(hook),
        )
        .await
        .unwrap();
        let input = serde_json::json!({
            "tool_name": "echo",
            "tool_input": { "message": "concurrent" }
        });

        let (first, second) = tokio::join!(
            session.invoke("alpha_alpha_invoke_tool", input.clone()),
            session.invoke("alpha_alpha_invoke_tool", input),
        );

        first.unwrap();
        second.unwrap();
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bridge_and_in_process_invocations_share_the_auth_refresh_lock() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("alpha_server.py");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let hook: BeforeExecHook = {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            Arc::new(move || {
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                Box::pin(async move {
                    let concurrent = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(concurrent, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        };
        let session = start_compressed_session_with_backend_configs_and_before_exec(
            FfiCompressedSessionConfig {
                compression_level: "max".to_string(),
                server_name: Some("alpha".to_string()),
                include_tools: Vec::new(),
                exclude_tools: Vec::new(),
                toonify: false,
                transform_mode: None,
                bridge: true,
            },
            vec![crate::server::BackendServerConfig::new(
                "alpha",
                python,
                [fixture.to_string_lossy().into_owned()],
            )],
            Some(hook),
        )
        .await
        .unwrap();
        let info = session.info();
        let bridge = reqwest::Client::new()
            .post(format!("{}/exec", info.bridge_url))
            .bearer_auth(info.token)
            .json(&serde_json::json!({
                "tool": "alpha_alpha_invoke_tool",
                "input": { "tool_name": "echo", "tool_input": { "message": "bridge" } }
            }))
            .send();
        let in_process = session.invoke(
            "alpha_alpha_invoke_tool",
            serde_json::json!({
                "tool_name": "echo",
                "tool_input": { "message": "in-process" }
            }),
        );

        let (bridge, in_process) = tokio::join!(bridge, in_process);

        bridge.unwrap().error_for_status().unwrap();
        in_process.unwrap();
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }
}
