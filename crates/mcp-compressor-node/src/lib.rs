use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use mcp_compressor_core::server::{BackendAuthMode, BackendServerConfig};
use napi::Error as NapiError;
use napi_derive::napi;
use serde::Deserialize;
use serde_json::Value;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::ffi::{
    build_host_transform_plan, clear_oauth_credentials, compress_tool_listing,
    format_tool_schema_response, generate_client_artifact_files, generate_client_artifacts,
    list_oauth_credentials, maybe_toonify_output, normalize_host_tool_result,
    normalize_sdk_servers, parse_mcp_config, parse_tool_argv, remember_oauth_backend,
    render_cli_subcommand_help, render_cli_top_level_help, start_compressed_session,
    start_compressed_session_from_mcp_config, FfiBackendConfig, FfiClientArtifactKind,
    FfiCompressedSession, FfiCompressedSessionConfig, FfiGeneratorConfig, FfiHostTransformConfig,
    FfiSdkServersConfig, FfiTool,
};

fn napi_error(error: impl std::fmt::Display) -> NapiError {
    NapiError::from_reason(error.to_string())
}

fn parse_json<T: for<'de> Deserialize<'de>>(value: &str) -> napi::Result<T> {
    serde_json::from_str(value).map_err(napi_error)
}

#[derive(Debug, Deserialize)]
struct ProviderBackendConfig {
    name: String,
    command_or_url: String,
    #[serde(default)]
    args: Vec<String>,
    provider_index: Option<usize>,
}

type HeaderStore = Arc<RwLock<BTreeMap<String, String>>>;

fn headers_from_store(
    store: HeaderStore,
) -> Result<BTreeMap<String, String>, mcp_compressor_core::Error> {
    store
        .read()
        .map(|headers| headers.clone())
        .map_err(|error| {
            mcp_compressor_core::Error::Config(format!("auth header store poisoned: {error}"))
        })
}

#[napi]
pub fn compress_tool_listing_json(level: String, tools_json: String) -> napi::Result<String> {
    let level = level.parse::<CompressionLevel>().map_err(napi_error)?;
    let tools = parse_json::<Vec<FfiTool>>(&tools_json)?;
    Ok(compress_tool_listing(level, tools))
}

#[napi]
pub fn format_tool_schema_response_json(tool_json: String) -> napi::Result<String> {
    let tool = parse_json::<FfiTool>(&tool_json)?;
    Ok(format_tool_schema_response(tool))
}

#[napi]
pub fn parse_tool_argv_json(tool_json: String, argv_json: String) -> napi::Result<String> {
    let tool = parse_json::<FfiTool>(&tool_json)?;
    let argv = parse_json::<Vec<String>>(&argv_json)?;
    let parsed = parse_tool_argv(tool, argv).map_err(napi_error)?;
    serde_json::to_string(&parsed).map_err(napi_error)
}

#[napi]
pub fn maybe_toonify_output_json(output: String) -> String {
    maybe_toonify_output(&output)
}

#[napi]
pub fn render_cli_top_level_help_json(
    command: String,
    cli_name: String,
    tools_json: String,
) -> napi::Result<String> {
    let tools = parse_json::<Vec<FfiTool>>(&tools_json)?;
    Ok(render_cli_top_level_help(command, cli_name, tools))
}

#[napi]
pub fn render_cli_subcommand_help_json(
    cli_name: String,
    tool_json: String,
) -> napi::Result<String> {
    let tool = parse_json::<FfiTool>(&tool_json)?;
    Ok(render_cli_subcommand_help(cli_name, tool))
}

fn parse_client_artifact_kind(kind: &str) -> napi::Result<FfiClientArtifactKind> {
    match kind {
        "cli" => Ok(FfiClientArtifactKind::Cli),
        "python" => Ok(FfiClientArtifactKind::Python),
        "typescript" | "ts" => Ok(FfiClientArtifactKind::TypeScript),
        other => Err(napi_error(format!("invalid client artifact kind: {other}"))),
    }
}

#[napi]
pub fn generate_client_artifacts_json(kind: String, config_json: String) -> napi::Result<String> {
    let kind = parse_client_artifact_kind(&kind)?;
    let config = parse_json::<FfiGeneratorConfig>(&config_json)?;
    let paths = generate_client_artifacts(kind, config).map_err(napi_error)?;
    let values = paths
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    serde_json::to_string(&values).map_err(napi_error)
}

#[napi]
pub fn generate_client_artifact_files_json(
    kind: String,
    config_json: String,
) -> napi::Result<String> {
    let kind = parse_client_artifact_kind(&kind)?;
    let config = parse_json::<FfiGeneratorConfig>(&config_json)?;
    let files = generate_client_artifact_files(kind, config).map_err(napi_error)?;
    serde_json::to_string(&files).map_err(napi_error)
}

#[napi]
pub fn build_host_transform_plan_json(config_json: String) -> napi::Result<String> {
    let config = parse_json::<FfiHostTransformConfig>(&config_json)?;
    let plan = build_host_transform_plan(config).map_err(napi_error)?;
    serde_json::to_string(&plan).map_err(napi_error)
}

#[napi]
pub fn normalize_host_tool_result_json(value_json: String, toonify: bool) -> napi::Result<String> {
    let value = parse_json::<Value>(&value_json)?;
    Ok(normalize_host_tool_result(value, toonify))
}

#[napi]
pub fn normalize_servers_json(servers_json: String) -> napi::Result<String> {
    let servers = parse_json::<FfiSdkServersConfig>(&servers_json)?;
    serde_json::to_string(&normalize_sdk_servers(servers).map_err(napi_error)?).map_err(napi_error)
}

#[napi]
pub fn parse_mcp_config_json(config_json: String) -> napi::Result<String> {
    let parsed = parse_mcp_config(&config_json).map_err(napi_error)?;
    serde_json::to_string(&parsed).map_err(napi_error)
}

#[napi]
pub fn remember_oauth_backend_json(
    backend_uri: String,
    backend_name: String,
    store_dir: String,
) -> napi::Result<()> {
    remember_oauth_backend(&backend_uri, &backend_name, store_dir.into()).map_err(napi_error)
}

#[napi]
pub fn list_oauth_credentials_json() -> napi::Result<String> {
    let entries = list_oauth_credentials().map_err(napi_error)?;
    serde_json::to_string(&entries).map_err(napi_error)
}

#[napi]
pub fn clear_oauth_credentials_json(target: Option<String>) -> napi::Result<String> {
    let paths = clear_oauth_credentials(target.as_deref()).map_err(napi_error)?;
    let values = paths
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    serde_json::to_string(&values).map_err(napi_error)
}

#[napi]
// codeql[cpp/access-invalid-pointer]: This exported napi class contains only Rust-owned fields.
// napi-rs generates the JS wrapper; this line does not dereference raw pointers.
pub struct NativeCompressedSession {
    inner: FfiCompressedSession,
    auth_header_stores: Vec<HeaderStore>,
}

#[napi]
impl NativeCompressedSession {
    #[napi]
    pub fn info_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner.info()).map_err(napi_error)
    }

    #[napi]
    pub fn close(&mut self) {}

    #[napi]
    pub fn update_auth_provider_headers_json(
        &self,
        provider_index: u32,
        headers_json: String,
    ) -> napi::Result<()> {
        let headers = parse_json::<BTreeMap<String, String>>(&headers_json)?;
        let store = self
            .auth_header_stores
            .get(provider_index as usize)
            .ok_or_else(|| {
                napi_error(format!(
                    "auth provider index out of range: {provider_index}"
                ))
            })?;
        let mut guard = store
            .write()
            .map_err(|error| napi_error(format!("auth header store poisoned: {error}")))?;
        *guard = headers;
        Ok(())
    }
}

#[napi]
pub async fn start_compressed_session_json(
    config_json: String,
    backends_json: String,
) -> napi::Result<NativeCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(&config_json)?;
    let backends = parse_json::<Vec<FfiBackendConfig>>(&backends_json)?;
    let inner = start_compressed_session(config, backends)
        .await
        .map_err(napi_error)?;
    Ok(NativeCompressedSession {
        inner,
        auth_header_stores: Vec::new(),
    })
}

#[napi]
pub async fn start_compressed_session_with_provider_backends_json(
    config_json: String,
    backends_json: String,
    providers_json: String,
) -> napi::Result<NativeCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(&config_json)?;
    let backends = parse_json::<Vec<ProviderBackendConfig>>(&backends_json)?;
    let providers = parse_json::<Vec<BTreeMap<String, String>>>(&providers_json)?
        .into_iter()
        .map(|headers| Arc::new(RwLock::new(headers)))
        .collect::<Vec<_>>();
    let mut backend_configs = Vec::new();
    for backend in backends {
        let mut config =
            BackendServerConfig::new(backend.name, backend.command_or_url, backend.args);
        if let Some(index) = backend.provider_index {
            let store =
                Arc::clone(providers.get(index).ok_or_else(|| {
                    napi_error(format!("auth provider index out of range: {index}"))
                })?);
            config = config
                .with_header_provider(Arc::new(move || headers_from_store(Arc::clone(&store))))
                .with_auth_mode(BackendAuthMode::ExplicitHeaders);
        }
        backend_configs.push(config);
    }
    let inner = mcp_compressor_core::ffi::start_compressed_session_with_backend_configs(
        config,
        backend_configs,
    )
    .await
    .map_err(napi_error)?;
    Ok(NativeCompressedSession {
        inner,
        auth_header_stores: providers,
    })
}

#[napi]
pub async fn start_compressed_session_from_mcp_config_json(
    config_json: String,
    mcp_config_json: String,
) -> napi::Result<NativeCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(&config_json)?;
    let inner = start_compressed_session_from_mcp_config(config, &mcp_config_json)
        .await
        .map_err(napi_error)?;
    Ok(NativeCompressedSession {
        inner,
        auth_header_stores: Vec::new(),
    })
}
