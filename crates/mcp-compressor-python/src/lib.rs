use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::ffi::{
    clear_oauth_credentials, compress_tool_listing, format_tool_schema_response, list_oauth_credentials,
    parse_mcp_config, parse_tool_argv, start_compressed_session,
    start_compressed_session_from_mcp_config, FfiBackendConfig, FfiCompressedSession,
    FfiCompressedSessionConfig, FfiTool,
};

fn py_value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn parse_json<T: for<'de> Deserialize<'de>>(value: &str) -> PyResult<T> {
    serde_json::from_str(value).map_err(py_value_error)
}

#[pyfunction]
fn compress_tool_listing_json(level: &str, tools_json: &str) -> PyResult<String> {
    let level = level.parse::<CompressionLevel>().map_err(py_value_error)?;
    let tools = parse_json::<Vec<FfiTool>>(tools_json)?;
    Ok(compress_tool_listing(level, tools))
}

#[pyfunction]
fn format_tool_schema_response_json(tool_json: &str) -> PyResult<String> {
    let tool = parse_json::<FfiTool>(tool_json)?;
    Ok(format_tool_schema_response(tool))
}

#[pyfunction]
fn parse_tool_argv_json(tool_json: &str, argv_json: &str) -> PyResult<String> {
    let tool = parse_json::<FfiTool>(tool_json)?;
    let argv = parse_json::<Vec<String>>(argv_json)?;
    let parsed = parse_tool_argv(tool, argv).map_err(py_value_error)?;
    serde_json::to_string(&parsed).map_err(py_value_error)
}

#[pyfunction]
fn parse_mcp_config_json(config_json: &str) -> PyResult<String> {
    let parsed = parse_mcp_config(config_json).map_err(py_value_error)?;
    serde_json::to_string(&parsed).map_err(py_value_error)
}

#[pyfunction]
fn list_oauth_credentials_json() -> PyResult<String> {
    let entries = list_oauth_credentials().map_err(py_value_error)?;
    serde_json::to_string(&entries).map_err(py_value_error)
}

#[pyclass]
struct PyCompressedSession {
    inner: FfiCompressedSession,
    #[allow(dead_code)]
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl PyCompressedSession {
    fn info_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.info()).map_err(py_value_error)
    }
}

#[pyfunction]
fn start_compressed_session_json(
    config_json: &str,
    backends_json: &str,
) -> PyResult<PyCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(config_json)?;
    let backends = parse_json::<Vec<FfiBackendConfig>>(backends_json)?;
    let runtime = tokio::runtime::Runtime::new().map_err(py_value_error)?;
    let inner = runtime
        .block_on(start_compressed_session(config, backends))
        .map_err(py_value_error)?;
    Ok(PyCompressedSession { inner, runtime })
}

#[pyfunction]
fn start_compressed_session_from_mcp_config_json(
    config_json: &str,
    mcp_config_json: &str,
) -> PyResult<PyCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(config_json)?;
    let runtime = tokio::runtime::Runtime::new().map_err(py_value_error)?;
    let inner = runtime
        .block_on(start_compressed_session_from_mcp_config(config, mcp_config_json))
        .map_err(py_value_error)?;
    Ok(PyCompressedSession { inner, runtime })
}

#[pyfunction]
fn clear_oauth_credentials_json(target: Option<&str>) -> PyResult<String> {
    let paths = clear_oauth_credentials(target).map_err(py_value_error)?;
    let values = paths
        .into_iter()
        .map(|path| Value::String(path.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    serde_json::to_string(&values).map_err(py_value_error)
}

#[pymodule]
fn _mcp_compressor_core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyCompressedSession>()?;
    module.add_function(wrap_pyfunction!(compress_tool_listing_json, module)?)?;
    module.add_function(wrap_pyfunction!(format_tool_schema_response_json, module)?)?;
    module.add_function(wrap_pyfunction!(parse_tool_argv_json, module)?)?;
    module.add_function(wrap_pyfunction!(parse_mcp_config_json, module)?)?;
    module.add_function(wrap_pyfunction!(start_compressed_session_json, module)?)?;
    module.add_function(wrap_pyfunction!(start_compressed_session_from_mcp_config_json, module)?)?;
    module.add_function(wrap_pyfunction!(list_oauth_credentials_json, module)?)?;
    module.add_function(wrap_pyfunction!(clear_oauth_credentials_json, module)?)?;
    Ok(())
}
