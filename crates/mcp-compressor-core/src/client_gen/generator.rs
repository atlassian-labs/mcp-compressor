//! `ClientGenerator` trait and `GeneratorConfig` shared across all generators.
//!
//! Generators render artifacts in memory. Callers can then either inspect the
//! generated file contents or persist them to `output_dir` with
//! `write_artifacts`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::compression::engine::Tool;
use crate::Error;

/// Inputs shared by all client generators.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Name of the CLI / library module to generate (e.g. `"confluence"`).
    pub cli_name: String,
    /// Base URL of the running tool proxy (e.g. `"http://127.0.0.1:51234"`).
    pub bridge_url: String,
    /// Session bearer token, embedded verbatim into every generated artifact.
    pub token: String,
    /// Upstream tools whose names and schemas drive artifact generation.
    pub tools: Vec<Tool>,
    /// PID of the proxy process, used for multi-session disambiguation.
    pub session_pid: u32,
    /// Directory where artifact files are written when persistence is requested.
    pub output_dir: PathBuf,
}

/// One generated client artifact held in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    pub file_name: String,
    pub contents: String,
    pub executable: bool,
}

impl GeneratedArtifact {
    pub fn new(file_name: impl Into<String>, contents: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            contents: contents.into(),
            executable: false,
        }
    }

    pub fn executable(mut self) -> Self {
        self.executable = true;
        self
    }
}

/// Trait implemented by every artifact generator.
pub trait ClientGenerator {
    /// Render artifact file(s) in memory.
    fn render(&self, config: &GeneratorConfig) -> Result<Vec<GeneratedArtifact>, Error>;

    /// Generate artifact file(s), write them to `config.output_dir`, and return their paths.
    fn generate(&self, config: &GeneratorConfig) -> Result<Vec<PathBuf>, Error> {
        let artifacts = self.render(config)?;
        write_artifacts(&artifacts, &config.output_dir)
    }
}

pub fn artifact_map(artifacts: &[GeneratedArtifact]) -> BTreeMap<String, String> {
    artifacts
        .iter()
        .map(|artifact| (artifact.file_name.clone(), artifact.contents.clone()))
        .collect()
}

pub fn write_artifacts(artifacts: &[GeneratedArtifact], output_dir: &Path) -> Result<Vec<PathBuf>, Error> {
    fs::create_dir_all(output_dir)?;
    let mut paths = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let path = output_dir.join(&artifact.file_name);
        fs::write(&path, &artifact.contents)?;
        #[cfg(unix)]
        if artifact.executable {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)?;
        }
        paths.push(path);
    }
    Ok(paths)
}

// ---------------------------------------------------------------------------
// Tests (shared contract verified against every implementation)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod test_helpers {
    //! Helpers reused by the individual generator test modules.

    use crate::compression::engine::Tool;
    use serde_json::json;
    use std::path::Path;

    use super::GeneratorConfig;

    /// Build a typical two-tool `GeneratorConfig` pointing at a temp dir.
    pub fn make_config(output_dir: &Path) -> GeneratorConfig {
        GeneratorConfig {
            cli_name: "my-server".to_string(),
            bridge_url: "http://127.0.0.1:51234".to_string(),
            token: "a3f7deadbeefa3f7deadbeefa3f7deadbeefa3f7deadbeefa3f7deadbeef1234".to_string(),
            tools: vec![
                Tool::new(
                    "fetch",
                    Some("Fetch a URL.".to_string()),
                    json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "URL to fetch."
                            },
                            "timeout": {
                                "type": "integer",
                                "description": "Timeout in seconds.",
                                "default": 30
                            },
                            "method": {
                                "type": "string",
                                "description": "HTTP method to use.",
                                "enum": ["GET", "POST"],
                                "default": "GET"
                            }
                        },
                        "required": ["url"]
                    }),
                ),
                Tool::new(
                    "search",
                    Some("Search the web.".to_string()),
                    json!({
                        "type": "object",
                        "properties": { "query": { "type": "string" } },
                        "required": ["query"]
                    }),
                ),
            ],
            session_pid: 12345,
            output_dir: output_dir.to_path_buf(),
        }
    }

    /// Build a config with a single multi-word tool name for naming tests.
    pub fn make_config_multiword_tool(output_dir: &Path) -> GeneratorConfig {
        let mut cfg = make_config(output_dir);
        cfg.tools = vec![Tool::new(
            "get_confluence_page",
            Some("Retrieve a Confluence page by ID.".to_string()),
            json!({
                "type": "object",
                "properties": {
                    "page_id":   { "type": "string" },
                    "space_key": { "type": "string" }
                },
                "required": ["page_id"]
            }),
        )];
        cfg
    }
}
