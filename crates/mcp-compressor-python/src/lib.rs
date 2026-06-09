use std::collections::BTreeMap;
use std::sync::Arc;

use mcp_compressor_core::server::{BackendAuthMode, BackendServerConfig};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::Deserialize;
use serde_json::Value;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::ffi::{
    clear_oauth_credentials, compress_tool_listing, format_tool_schema_response,
    generate_client_artifact_files, generate_client_artifacts, list_oauth_credentials, normalize_sdk_servers, parse_mcp_config,
    parse_tool_argv, start_compressed_session, start_compressed_session_from_mcp_config,
    FfiBackendConfig, FfiClientArtifactKind, FfiCompressedSession, FfiCompressedSessionConfig,
    FfiGeneratorConfig, FfiSdkServersConfig, FfiTool,
};

fn py_value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn parse_json<T: for<'de> Deserialize<'de>>(value: &str) -> PyResult<T> {
    serde_json::from_str(value).map_err(py_value_error)
}

#[derive(Debug, Deserialize)]
struct ProviderBackendConfig {
    name: String,
    command_or_url: String,
    #[serde(default)]
    args: Vec<String>,
    provider_index: Option<usize>,
}

fn provider_headers_from_python(provider: &Py<PyAny>) -> Result<BTreeMap<String, String>, mcp_compressor_core::Error> {
    Python::attach(|py| {
        let value = provider
            .call0(py)
            .map_err(|error| mcp_compressor_core::Error::Config(error.to_string()))?;
        let dict = value
            .downcast_bound::<PyDict>(py)
            .map_err(|_| mcp_compressor_core::Error::Config("auth_provider must return a dict".to_string()))?;
        let mut headers = BTreeMap::new();
        for (key, value) in dict.iter() {
            headers.insert(
                key.extract::<String>()
                    .map_err(|error| mcp_compressor_core::Error::Config(error.to_string()))?,
                value
                    .extract::<String>()
                    .map_err(|error| mcp_compressor_core::Error::Config(error.to_string()))?,
            );
        }
        Ok(headers)
    })
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

fn parse_client_artifact_kind(kind: &str) -> PyResult<FfiClientArtifactKind> {
    match kind {
        "cli" => Ok(FfiClientArtifactKind::Cli),
        "python" => Ok(FfiClientArtifactKind::Python),
        "typescript" => Ok(FfiClientArtifactKind::TypeScript),
        other => Err(py_value_error(format!("unsupported client artifact kind: {other}"))),
    }
}

#[pyfunction]
fn generate_client_artifacts_json(kind: &str, config_json: &str) -> PyResult<String> {
    let kind = parse_client_artifact_kind(kind)?;
    let config = parse_json::<FfiGeneratorConfig>(config_json)?;
    let paths = generate_client_artifacts(kind, config).map_err(py_value_error)?;
    serde_json::to_string(&paths.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>())
        .map_err(py_value_error)
}

#[pyfunction]
fn generate_client_artifact_files_json(kind: &str, config_json: &str) -> PyResult<String> {
    let kind = parse_client_artifact_kind(kind)?;
    let config = parse_json::<FfiGeneratorConfig>(config_json)?;
    let files = generate_client_artifact_files(kind, config).map_err(py_value_error)?;
    serde_json::to_string(&files).map_err(py_value_error)
}

#[pyfunction]
fn normalize_servers_json(servers_json: &str) -> PyResult<String> {
    let servers: FfiSdkServersConfig = serde_json::from_str(servers_json).map_err(py_value_error)?;
    serde_json::to_string(&normalize_sdk_servers(servers).map_err(py_value_error)?)
        .map_err(py_value_error)
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

    /// In-process compressed frontend tool list (no HTTP bridge required).
    fn list_frontend_tools_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner.list_frontend_tools()).map_err(py_value_error)
    }

    /// In-process schema lookup for a backend tool via a compressed wrapper.
    fn get_tool_schema_json(
        &self,
        py: Python<'_>,
        wrapper_tool_name: &str,
        backend_tool_name: &str,
    ) -> PyResult<String> {
        py.detach(|| {
            self.runtime
                .block_on(self.inner.get_tool_schema(wrapper_tool_name, backend_tool_name))
        })
        .map_err(py_value_error)
    }

    /// In-process tool invocation, reusing the session's connection and OAuth.
    ///
    /// `input_json` is the JSON-encoded tool input (for wrapper invoke tools,
    /// an object with `tool_name` and `tool_input`).
    fn invoke_tool_json(&self, py: Python<'_>, tool: &str, input_json: &str) -> PyResult<String> {
        let input: Value = parse_json(input_json)?;
        py.detach(|| self.runtime.block_on(self.inner.invoke(tool, input)))
            .map_err(py_value_error)
    }

    fn close(&mut self) {
        // The Rust session/proxy shuts down when the Python object is dropped.
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
fn start_compressed_session_with_provider_backends_json(
    py: Python<'_>,
    config_json: &str,
    backends_json: &str,
    providers: Vec<Py<PyAny>>,
) -> PyResult<PyCompressedSession> {
    let config = parse_json::<FfiCompressedSessionConfig>(config_json)?;
    let backends = parse_json::<Vec<ProviderBackendConfig>>(backends_json)?;
    let mut backend_configs = Vec::new();
    for backend in backends {
        let mut config = BackendServerConfig::new(backend.name, backend.command_or_url, backend.args);
        if let Some(index) = backend.provider_index {
            let provider = Python::attach(|py| {
                providers
                    .get(index)
                    .ok_or_else(|| py_value_error(format!("auth provider index out of range: {index}")))
                    .map(|provider| provider.clone_ref(py))
            })?;
            config = config
                .with_header_provider(Arc::new(move || provider_headers_from_python(&provider)))
                .with_auth_mode(BackendAuthMode::ExplicitHeaders);
        }
        backend_configs.push(config);
    }
    let runtime = tokio::runtime::Runtime::new().map_err(py_value_error)?;
    let inner = py
        .detach(|| {
            runtime.block_on(mcp_compressor_core::ffi::start_compressed_session_with_backend_configs(
                config,
                backend_configs,
            ))
        })
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

#[pyfunction]
fn run_cli_json(argv_json: &str) -> PyResult<i32> {
    let argv: Vec<String> = parse_json(argv_json)?;
    match mcp_compressor_core::app::entrypoint::run_from(argv) {
        Ok(()) => Ok(0),
        Err(mcp_compressor_core::app::entrypoint::CliError::Display(message)) => {
            print!("{message}");
            Ok(0)
        }
        Err(mcp_compressor_core::app::entrypoint::CliError::Usage(message)) => {
            eprintln!("error: {message}");
            Ok(2)
        }
        Err(mcp_compressor_core::app::entrypoint::CliError::Runtime(message)) => {
            eprintln!("error: {message}");
            Ok(1)
        }
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyCompressedSession>()?;
    module.add_function(wrap_pyfunction!(compress_tool_listing_json, module)?)?;
    module.add_function(wrap_pyfunction!(format_tool_schema_response_json, module)?)?;
    module.add_function(wrap_pyfunction!(parse_tool_argv_json, module)?)?;
    module.add_function(wrap_pyfunction!(generate_client_artifacts_json, module)?)?;
    module.add_function(wrap_pyfunction!(generate_client_artifact_files_json, module)?)?;
    module.add_function(wrap_pyfunction!(normalize_servers_json, module)?)?;
    module.add_function(wrap_pyfunction!(parse_mcp_config_json, module)?)?;
    module.add_function(wrap_pyfunction!(start_compressed_session_json, module)?)?;
    module.add_function(wrap_pyfunction!(start_compressed_session_with_provider_backends_json, module)?)?;
    module.add_function(wrap_pyfunction!(start_compressed_session_from_mcp_config_json, module)?)?;
    module.add_function(wrap_pyfunction!(list_oauth_credentials_json, module)?)?;
    module.add_function(wrap_pyfunction!(clear_oauth_credentials_json, module)?)?;
    module.add_function(wrap_pyfunction!(run_cli_json, module)?)?;
    Ok(())
}
