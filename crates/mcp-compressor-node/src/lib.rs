use napi::Error as NapiError;
use napi_derive::napi;
use serde::Deserialize;
use serde_json::Value;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::ffi::{
    clear_oauth_credentials, compress_tool_listing, format_tool_schema_response, generate_client_artifacts,
    list_oauth_credentials, parse_mcp_config, parse_tool_argv, remember_oauth_backend,
    start_compressed_session,
    start_compressed_session_from_mcp_config, FfiBackendConfig, FfiClientArtifactKind,
    FfiCompressedSession, FfiCompressedSessionConfig, FfiGeneratorConfig, FfiTool,
};

fn napi_error(error: impl std::fmt::Display) -> NapiError {
    NapiError::from_reason(error.to_string())
}

fn parse_json<T: for<'de> Deserialize<'de>>(value: &str) -> napi::Result<T> {
    serde_json::from_str(value).map_err(napi_error)
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
pub fn generate_client_artifacts_json(kind: String, config_json: String) -> napi::Result<String> {
    let kind = match kind.as_str() {
        "cli" => FfiClientArtifactKind::Cli,
        "python" => FfiClientArtifactKind::Python,
        "typescript" | "ts" => FfiClientArtifactKind::TypeScript,
        other => return Err(napi_error(format!("invalid client artifact kind: {other}"))),
    };
    let config = parse_json::<FfiGeneratorConfig>(&config_json)?;
    let paths = generate_client_artifacts(kind, config).map_err(napi_error)?;
    let values = paths
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    serde_json::to_string(&values).map_err(napi_error)
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
pub struct NativeCompressedSession {
    inner: FfiCompressedSession,
}

#[napi]
impl NativeCompressedSession {
    #[napi]
    pub fn info_json(&self) -> napi::Result<String> {
        serde_json::to_string(&self.inner.info()).map_err(napi_error)
    }
}

#[napi]
pub async fn start_compressed_session_json(
    config_json: String,
    backends_json: String,
) -> napi::Result<NativeCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(&config_json)?;
    let backends = parse_json::<Vec<FfiBackendConfig>>(&backends_json)?;
    let inner = start_compressed_session(config, backends).await.map_err(napi_error)?;
    Ok(NativeCompressedSession { inner })
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
    Ok(NativeCompressedSession { inner })
}
