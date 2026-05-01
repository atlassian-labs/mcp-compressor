//! MCP config JSON parsing and server-topology helpers.
//!
//! Supports the standard `mcpServers` JSON format used by Claude Desktop,
//! VS Code, and other MCP host applications:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "my-server": {
//!       "command": "uvx",
//!       "args": ["mcp-server-fetch"],
//!       "env": { "API_KEY": "secret" }
//!     }
//!   }
//! }
//! ```

use std::collections::HashMap;

use crate::cli::mapping::sanitize_cli_name;
use crate::Error;

/// Configuration for a single MCP backend server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerConfig {
    /// The executable to launch (e.g. `"uvx"`, `"npx"`, `"node"`).
    pub command: String,
    /// Arguments passed to `command` (may be absent — defaults to empty).
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables injected into the server process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Parsed representation of an MCP host config file.
#[derive(Debug, Clone)]
pub struct MCPConfig {
    servers: HashMap<String, ServerConfig>,
}

impl MCPConfig {
    /// Parse an MCP config JSON string.
    ///
    /// Returns an error when the JSON is malformed or when the `mcpServers`
    /// key is absent.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawConfig {
            mcp_servers: HashMap<String, ServerConfig>,
        }

        let raw: RawConfig = serde_json::from_str(json)?;
        Ok(Self { servers: raw.mcp_servers })
    }

    /// Return server names in ascending lexicographic order.
    pub fn server_names(&self) -> Vec<String> {
        let mut names = self.servers.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Look up a server configuration by name.
    pub fn server(&self, name: &str) -> Option<&ServerConfig> {
        self.servers.get(name)
    }

    /// Return the CLI prefix (sanitized server name) for a given server.
    ///
    /// Used to namespace subcommands in multi-server CLI mode.
    pub fn cli_prefix(&self, server_name: &str) -> String {
        sanitize_cli_name(server_name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Single-server configs
    // ------------------------------------------------------------------

    /// A minimal valid single-server config is parsed without error.
    #[test]
    fn parse_single_server() {
        let json = r#"{"mcpServers": {"my-server": {"command": "uvx", "args": ["my-server"]}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        assert_eq!(config.server_names(), vec!["my-server"]);
    }

    /// The parsed server has the correct command and args.
    #[test]
    fn single_server_command_and_args() {
        let json = r#"{"mcpServers": {"s": {"command": "uvx", "args": ["mcp-fetch"]}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        let server = config.server("s").unwrap();
        assert_eq!(server.command, "uvx");
        assert_eq!(server.args, vec!["mcp-fetch"]);
    }

    /// A server with no `args` key defaults to an empty arg list.
    #[test]
    fn server_without_args_defaults_to_empty() {
        let json = r#"{"mcpServers": {"s": {"command": "uvx"}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        let server = config.server("s").unwrap();
        assert!(server.args.is_empty());
    }

    /// `env` entries are parsed into the server config map.
    #[test]
    fn server_env_vars_parsed() {
        let json = r#"{
            "mcpServers": {
                "s": {
                    "command": "uvx",
                    "args": [],
                    "env": { "API_KEY": "secret", "DEBUG": "1" }
                }
            }
        }"#;
        let config = MCPConfig::from_json(json).unwrap();
        let server = config.server("s").unwrap();
        assert_eq!(server.env.get("API_KEY"), Some(&"secret".to_string()));
        assert_eq!(server.env.get("DEBUG"), Some(&"1".to_string()));
    }

    /// A server with no `env` key defaults to an empty map.
    #[test]
    fn server_without_env_defaults_to_empty() {
        let json = r#"{"mcpServers": {"s": {"command": "cmd"}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        let server = config.server("s").unwrap();
        assert!(server.env.is_empty());
    }

    // ------------------------------------------------------------------
    // Multi-server configs
    // ------------------------------------------------------------------

    /// A config with multiple servers is parsed and all names are present.
    #[test]
    fn parse_multi_server() {
        let json = r#"{
            "mcpServers": {
                "server-a": {"command": "uvx", "args": ["a"]},
                "server-b": {"command": "npx", "args": ["-y", "b"]}
            }
        }"#;
        let config = MCPConfig::from_json(json).unwrap();
        let names = config.server_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"server-a".to_string()));
        assert!(names.contains(&"server-b".to_string()));
    }

    /// `server_names()` returns names in ascending lexicographic order.
    #[test]
    fn server_names_sorted() {
        let json = r#"{
            "mcpServers": {
                "zebra-server": {"command": "z"},
                "alpha-server": {"command": "a"},
                "mango-server": {"command": "m"}
            }
        }"#;
        let config = MCPConfig::from_json(json).unwrap();
        assert_eq!(config.server_names(), vec!["alpha-server", "mango-server", "zebra-server"]);
    }

    // ------------------------------------------------------------------
    // Empty server list
    // ------------------------------------------------------------------

    /// An empty `mcpServers` object is valid and yields no servers.
    #[test]
    fn empty_server_list() {
        let json = r#"{"mcpServers": {}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        assert_eq!(config.server_names(), Vec::<String>::new());
    }

    // ------------------------------------------------------------------
    // Error cases
    // ------------------------------------------------------------------

    /// Malformed JSON returns an error.
    #[test]
    fn invalid_json_is_error() {
        assert!(MCPConfig::from_json("{invalid}").is_err());
    }

    /// Completely empty input returns an error.
    #[test]
    fn empty_input_is_error() {
        assert!(MCPConfig::from_json("").is_err());
    }

    /// A JSON object missing the `mcpServers` key returns an error.
    #[test]
    fn missing_mcp_servers_key_is_error() {
        assert!(MCPConfig::from_json(r#"{"servers": {}}"#).is_err());
    }

    // ------------------------------------------------------------------
    // server() lookup
    // ------------------------------------------------------------------

    /// `server()` returns None for a name that does not exist.
    #[test]
    fn server_lookup_missing_name() {
        let json = r#"{"mcpServers": {"s": {"command": "cmd"}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        assert!(config.server("nonexistent").is_none());
    }

    // ------------------------------------------------------------------
    // cli_prefix
    // ------------------------------------------------------------------

    /// `cli_prefix` returns the sanitized server name.
    #[test]
    fn cli_prefix_returns_sanitized_name() {
        let json = r#"{"mcpServers": {"my-server": {"command": "cmd"}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        // "my-server" is already a valid CLI name
        assert_eq!(config.cli_prefix("my-server"), "my-server");
    }

    /// `cli_prefix` sanitizes server names with special characters.
    #[test]
    fn cli_prefix_sanitizes_name() {
        let json = r#"{"mcpServers": {"My Server!": {"command": "cmd"}}}"#;
        let config = MCPConfig::from_json(json).unwrap();
        // "My Server!" → "my-server" (via sanitize_cli_name rules)
        assert_eq!(config.cli_prefix("My Server!"), "my-server");
    }
}
