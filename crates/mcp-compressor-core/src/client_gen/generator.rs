//! `ClientGenerator` trait and `GeneratorConfig` shared across all generators.
//!
//! Every generator receives the same `GeneratorConfig` and writes one or more
//! artifact files to `output_dir`.  The generated files embed the session
//! `token` and `bridge_url` so that tool calls are authenticated against the
//! correct proxy process.

use std::path::PathBuf;

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
    /// Directory where artifact files are written.
    pub output_dir: PathBuf,
}

/// Trait implemented by every artifact generator.
///
/// `generate` writes its artifacts to `config.output_dir` and returns the
/// list of paths it created.  Implementations must be pure (no network,
/// no subprocess) — file I/O only.
pub trait ClientGenerator {
    /// Generate artifact file(s) and return their paths.
    fn generate(&self, config: &GeneratorConfig) -> Result<Vec<PathBuf>, Error>;
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
                        "properties": { "url": { "type": "string" } },
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
