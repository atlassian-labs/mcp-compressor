//! Optional napi-rs binding scaffold for the Rust core.
//!
//! Like the PyO3 scaffold, this starts with a JSON-string based surface so the
//! TypeScript package can wrap stable Rust behavior without committing to final
//! packaging details yet.

use napi::Error as NapiError;
use napi_derive::napi;
use serde::Deserialize;
use serde_json::Value;

use crate::compression::CompressionLevel;
use crate::ffi::types::{
    clear_oauth_credentials, compress_tool_listing, format_tool_schema_response,
    list_oauth_credentials, parse_mcp_config, parse_tool_argv, FfiTool,
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
pub fn parse_mcp_config_json(config_json: String) -> napi::Result<String> {
    let parsed = parse_mcp_config(&config_json).map_err(napi_error)?;
    serde_json::to_string(&parsed).map_err(napi_error)
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
