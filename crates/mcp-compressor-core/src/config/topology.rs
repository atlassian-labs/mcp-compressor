//! MCP config JSON parsing and server-topology helpers.
//!
//! Supports both the `mcpServers` and `servers` JSON envelopes used by MCP host
//! applications; MCP host configuration envelopes are not protocol-standard:
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
use crate::server::backend::interpolate_env;
use crate::server::BackendServerConfig;
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

#[derive(Debug, Clone, Default)]
struct ServerOptions {
    headers: HashMap<String, String>,
    oauth_app_name: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParsedServerConfig {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    oauth_app_name: Option<String>,
}

impl ParsedServerConfig {
    fn into_parts(self, name: &str) -> Result<(ServerConfig, ServerOptions), Error> {
        let command = match (self.command, self.url) {
            (Some(command), None) => command,
            (None, Some(url)) => normalize_http_url(name, &url)?,
            _ => {
                return Err(Error::Config(format!(
                    "server {name} must define exactly one of command or url"
                )))
            }
        };
        Ok((
            ServerConfig {
                command,
                args: self.args,
                env: self.env,
            },
            ServerOptions {
                headers: self.headers,
                oauth_app_name: self.oauth_app_name,
            },
        ))
    }
}

fn normalize_http_url(name: &str, value: &str) -> Result<String, Error> {
    let url = reqwest::Url::parse(value).map_err(|error| {
        Error::Config(format!(
            "server {name} url must be a valid HTTP(S) URL: {error}"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Config(format!(
            "server {name} url must be a valid HTTP(S) URL"
        )));
    }
    Ok(url.to_string())
}

impl ServerConfig {
    fn into_backend(self, name: String, options: ServerOptions) -> BackendServerConfig {
        let mut backend = BackendServerConfig::new(name, self.command, self.args);
        // `BackendServerConfig::new` also parses `-H`/`--env` flags out of
        // `args`, so the explicit config maps have to be merged on top of the
        // parsed ones instead of replacing them.
        let mut env = backend.env.clone();
        env.extend(
            self.env
                .into_iter()
                .map(|(key, value)| (key, interpolate_env(&value))),
        );
        let mut headers = backend.headers.clone();
        for (name, value) in options.headers {
            headers.retain(|existing, _| !existing.eq_ignore_ascii_case(&name));
            headers.insert(name, interpolate_env(&value));
        }
        backend = backend.with_env(env).with_headers(headers);
        // The auth mode stays `Auto` so that, exactly as on the CLI, only an
        // `Authorization` header disables OAuth. Non-auth headers such as
        // `X-Trace-Id` must not silently opt the backend out of login.
        if let Some(app_name) = options.oauth_app_name {
            backend = backend.with_oauth_app_name(app_name);
        }
        backend
    }
}

/// Parsed representation of an MCP host config file.
#[derive(Debug, Clone)]
pub struct MCPConfig {
    servers: HashMap<String, ServerConfig>,
    options: HashMap<String, ServerOptions>,
}

impl MCPConfig {
    /// Parse an MCP config JSON string.
    ///
    /// Returns an error when the JSON is malformed or does not contain exactly
    /// one supported server envelope.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawConfig {
            #[serde(default)]
            mcp_servers: Option<HashMap<String, ParsedServerConfig>>,
            #[serde(default)]
            servers: Option<HashMap<String, ParsedServerConfig>>,
        }

        let raw: RawConfig = serde_json::from_str(json)?;
        let parsed_servers = match (raw.mcp_servers, raw.servers) {
            (Some(servers), None) | (None, Some(servers)) => servers,
            _ => {
                return Err(Error::Config(
                    "config must define exactly one of mcpServers or servers".to_string(),
                ))
            }
        };
        let mut servers = HashMap::new();
        let mut options = HashMap::new();
        for (name, parsed) in parsed_servers {
            let (server, server_options) = parsed.into_parts(&name)?;
            servers.insert(name.clone(), server);
            options.insert(name, server_options);
        }
        Ok(Self { servers, options })
    }

    pub(crate) fn into_backend_configs(self) -> Result<Vec<BackendServerConfig>, Error> {
        let mut servers = self.servers.into_iter().collect::<Vec<_>>();
        servers.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok(servers
            .into_iter()
            .map(|(name, server)| {
                let options = self.options.get(&name).cloned().unwrap_or_default();
                server.into_backend(name, options)
            })
            .collect())
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

    pub(crate) fn server_metadata(
        &self,
        name: &str,
    ) -> Option<(&HashMap<String, String>, Option<&str>)> {
        self.options
            .get(name)
            .map(|options| (&options.headers, options.oauth_app_name.as_deref()))
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

    #[test]
    fn command_backend_preserves_args_and_env() {
        let json = r#"{
            "mcpServers": {
                "local": {
                    "command": "python",
                    "args": ["server.py"],
                    "env": { "API_KEY": "test-value" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.command, "python");
        assert_eq!(backend.args, ["server.py"]);
        assert_eq!(backend.env["API_KEY"], "test-value");
    }

    #[test]
    fn url_backend_preserves_headers_and_disables_oauth() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "headers": { "Authorization": "Bearer test-token" },
                    "oauthAppName": "Test Agent"
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.command, "https://example.test/mcp");
        assert_eq!(backend.headers["Authorization"], "Bearer test-token");
        assert_eq!(backend.oauth_app_name.as_deref(), Some("Test Agent"));
        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn url_backend_without_headers_uses_oauth() {
        let json = r#"{"mcpServers":{"remote":{"url":"https://example.test/mcp"}}}"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert!(backend.should_use_oauth());
    }

    /// Only an `Authorization` header opts a remote backend out of OAuth. A
    /// tracing or tenant header must not silently disable login, which is how
    /// the same backend behaves when configured on the CLI.
    #[test]
    fn url_backend_with_non_auth_headers_still_uses_oauth() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "headers": { "X-Trace-Id": "abc123" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.headers["X-Trace-Id"], "abc123");
        assert!(backend.should_use_oauth());
    }

    #[test]
    fn url_backend_can_force_oauth_from_json_args() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "args": ["--auth", "oauth"],
                    "headers": { "Authorization": "Bearer static-token" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.auth_mode, crate::server::BackendAuthMode::OAuth);
        assert!(backend.should_use_oauth());
    }

    #[test]
    fn url_backend_can_force_explicit_headers_from_json_args() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "args": ["--auth=explicit-headers"]
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(
            backend.auth_mode,
            crate::server::BackendAuthMode::ExplicitHeaders
        );
        assert!(!backend.should_use_oauth());
    }

    /// `BackendServerConfig::new` parses `-H` and `--env` out of `args`, so the
    /// explicit config maps must merge with them rather than wipe them.
    #[test]
    fn config_maps_merge_with_values_parsed_from_args() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "args": ["-H", "X-From-Args=args-value"],
                    "headers": { "X-From-Config": "config-value" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.headers["X-From-Args"], "args-value");
        assert_eq!(backend.headers["X-From-Config"], "config-value");
    }

    #[test]
    fn explicit_headers_override_parsed_headers_case_insensitively() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "args": ["-H", "authorization=stale"],
                    "headers": { "Authorization": "current" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.headers.len(), 1);
        assert_eq!(backend.headers["Authorization"], "current");
    }

    /// Env values are interpolated exactly like header values and like the
    /// TypeScript config loader, so `${VAR}` placeholders resolve.
    #[test]
    fn config_env_values_interpolate_environment_variables() {
        std::env::set_var("MCP_COMPRESSOR_TOPOLOGY_TEST", "resolved");
        let json = r#"{
            "mcpServers": {
                "local": {
                    "command": "python",
                    "env": { "TOKEN": "${MCP_COMPRESSOR_TOPOLOGY_TEST}" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(backend.env["TOKEN"], "resolved");
    }

    #[test]
    fn config_header_values_interpolate_environment_variables_once() {
        std::env::set_var(
            "MCP_COMPRESSOR_HEADER_OUTER_TEST",
            "${MCP_COMPRESSOR_HEADER_INNER_TEST}",
        );
        std::env::set_var("MCP_COMPRESSOR_HEADER_INNER_TEST", "expanded twice");
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "url": "https://example.test/mcp",
                    "headers": { "X-Token": "${MCP_COMPRESSOR_HEADER_OUTER_TEST}" }
                }
            }
        }"#;
        let backend = MCPConfig::from_json(json)
            .unwrap()
            .into_backend_configs()
            .unwrap()
            .remove(0);

        assert_eq!(
            backend.headers["X-Token"],
            "${MCP_COMPRESSOR_HEADER_INNER_TEST}"
        );
    }

    #[test]
    fn server_requires_exactly_one_transport() {
        for json in [
            r#"{"mcpServers":{"invalid":{}}}"#,
            r#"{"mcpServers":{"invalid":{"command":"python","url":"https://example.test/mcp"}}}"#,
        ] {
            let error = MCPConfig::from_json(json).unwrap_err();
            assert!(error
                .to_string()
                .contains("must define exactly one of command or url"));
        }
    }

    #[test]
    fn url_backend_rejects_invalid_or_non_http_urls() {
        for url in ["", "not-a-url", "ftp://example.test/mcp"] {
            let json = serde_json::json!({
                "mcpServers": { "remote": { "url": url } }
            });
            let error = MCPConfig::from_json(&json.to_string()).unwrap_err();
            assert!(
                error.to_string().contains("valid HTTP(S) URL"),
                "url {url:?}: {error}"
            );
        }
    }

    #[test]
    fn url_backend_normalizes_case_insensitive_http_scheme() {
        let backend =
            MCPConfig::from_json(r#"{"mcpServers":{"remote":{"url":"HTTP://example.test/mcp"}}}"#)
                .unwrap()
                .into_backend_configs()
                .unwrap()
                .remove(0);

        assert_eq!(
            backend.transport,
            crate::server::BackendTransport::StreamableHttp
        );
        assert_eq!(backend.command, "http://example.test/mcp");
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
        assert_eq!(
            config.server_names(),
            vec!["alpha-server", "mango-server", "zebra-server"]
        );
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

    #[test]
    fn servers_envelope_is_accepted() {
        let config =
            MCPConfig::from_json(r#"{"servers":{"remote":{"url":"https://example.test/mcp"}}}"#)
                .unwrap();

        assert_eq!(config.server_names(), ["remote"]);
    }

    #[test]
    fn empty_servers_envelope_is_accepted() {
        let config = MCPConfig::from_json(r#"{"servers":{}}"#).unwrap();

        assert!(config.server_names().is_empty());
    }

    #[test]
    fn missing_server_envelope_is_error() {
        let error = MCPConfig::from_json(r#"{}"#).unwrap_err();

        assert!(error
            .to_string()
            .contains("exactly one of mcpServers or servers"));
    }

    #[test]
    fn multiple_server_envelopes_are_error() {
        let error = MCPConfig::from_json(r#"{"mcpServers":{},"servers":{}}"#).unwrap_err();

        assert!(error
            .to_string()
            .contains("exactly one of mcpServers or servers"));
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
