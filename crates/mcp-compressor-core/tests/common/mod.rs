use std::path::PathBuf;

use mcp_compressor_core::compression::CompressionLevel;
use mcp_compressor_core::server::{
    BackendConfigSource, BackendServerConfig, CompressedServerConfig, ProxyTransformMode,
};

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn python_command() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string())
}

pub fn backend(name: &str, fixture: &str) -> BackendServerConfig {
    BackendServerConfig::new(
        name,
        python_command(),
        [fixture_path(fixture).to_string_lossy().into_owned()],
    )
}

pub fn config(
    level: CompressionLevel,
    server_name: impl Into<Option<&'static str>>,
    transform_mode: ProxyTransformMode,
    config_source: BackendConfigSource,
) -> CompressedServerConfig {
    CompressedServerConfig {
        level,
        server_name: server_name.into().map(str::to_string),
        include_tools: Vec::new(),
        exclude_tools: Vec::new(),
        toonify: false,
        transform_mode,
        config_source,
    }
}

pub fn max_config(server_name: impl Into<Option<&'static str>>) -> CompressedServerConfig {
    config(
        CompressionLevel::Max,
        server_name,
        ProxyTransformMode::CompressedTools,
        BackendConfigSource::Command,
    )
}

pub fn mcp_config_json(backends: &[(&str, &str)]) -> String {
    let servers = backends
        .iter()
        .map(|(name, fixture)| {
            let path = fixture_path(fixture).to_string_lossy().into_owned();
            format!(
                r#""{name}":{{"command":"{}","args":["{}"]}}"#,
                python_command(),
                path
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"mcpServers":{{{servers}}}}}"#)
}
