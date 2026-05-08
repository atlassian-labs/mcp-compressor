use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use axum::http::{HeaderName, HeaderValue};

use crate::Error;

/// Transport type used to reach an upstream MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTransport {
    /// Spawn a local command and speak MCP over stdio.
    Stdio,
    /// Connect to a remote streamable HTTP MCP endpoint.
    StreamableHttp,
}

/// Authentication strategy for a remote upstream MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendAuthMode {
    /// Match Python parity: explicit `Authorization` headers are used as-is;
    /// otherwise native OAuth should be attempted for remote HTTP backends.
    Auto,
    /// Use explicit backend headers only; never start OAuth.
    ExplicitHeaders,
    /// Force native OAuth.
    OAuth,
}

impl Default for BackendAuthMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Configuration for one upstream MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub transport: BackendTransport,
    pub headers: HashMap<String, String>,
    pub auth_mode: BackendAuthMode,
}

impl BackendServerConfig {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let command = command.into();
        let transport = if is_http_url(&command) {
            BackendTransport::StreamableHttp
        } else {
            BackendTransport::Stdio
        };
        let raw_args = args.into_iter().map(Into::into).collect::<Vec<_>>();
        let parsed_args = parse_backend_args(raw_args, transport);
        Self {
            name: name.into(),
            command,
            args: parsed_args.args,
            env: parsed_args.env,
            cwd: parsed_args.cwd,
            timeout: parsed_args.timeout,
            transport,
            headers: parsed_args.headers,
            auth_mode: parsed_args.auth_mode,
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_headers(
        mut self,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.headers = headers
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect();
        self
    }

    pub fn with_auth_mode(mut self, auth_mode: BackendAuthMode) -> Self {
        self.auth_mode = auth_mode;
        self
    }

    pub fn has_authorization_header(&self) -> bool {
        self.headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    }

    pub fn should_use_oauth(&self) -> bool {
        self.transport == BackendTransport::StreamableHttp
            && match self.auth_mode {
                BackendAuthMode::Auto => !self.has_authorization_header(),
                BackendAuthMode::ExplicitHeaders => false,
                BackendAuthMode::OAuth => true,
            }
    }
}

pub fn backend_http_headers(
    backend: &BackendServerConfig,
) -> Result<HashMap<HeaderName, HeaderValue>, Error> {
    backend
        .headers
        .iter()
        .map(|(name, value)| {
            let name = HeaderName::from_str(name).map_err(|error| {
                Error::Config(format!("invalid HTTP header name {name:?}: {error}"))
            })?;
            let value = HeaderValue::from_str(value).map_err(|error| {
                Error::Config(format!("invalid HTTP header value for {name:?}: {error}"))
            })?;
            Ok((name, value))
        })
        .collect()
}

#[derive(Debug, Default)]
struct ParsedBackendArgs {
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    timeout: Option<Duration>,
    headers: HashMap<String, String>,
    auth_mode: BackendAuthMode,
}

fn parse_backend_args(args: Vec<String>, transport: BackendTransport) -> ParsedBackendArgs {
    let mut parsed = ParsedBackendArgs {
        auth_mode: BackendAuthMode::Auto,
        ..Default::default()
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-H" || arg == "--header" {
            if let Some(header) = args.get(index + 1) {
                if transport == BackendTransport::StreamableHttp {
                    if let Some((name, value)) = parse_header_arg(header) {
                        parsed.headers.insert(name, value);
                    } else {
                        parsed.args.push(arg.clone());
                        parsed.args.push(header.clone());
                    }
                } else {
                    parsed.args.push(arg.clone());
                    parsed.args.push(header.clone());
                }
                index += 2;
            } else {
                parsed.args.push(arg.clone());
                index += 1;
            }
        } else if let Some(header) = arg
            .strip_prefix("-H=")
            .or_else(|| arg.strip_prefix("--header="))
        {
            if transport == BackendTransport::StreamableHttp {
                if let Some((name, value)) = parse_header_arg(header) {
                    parsed.headers.insert(name, value);
                } else {
                    parsed.args.push(arg.clone());
                }
            } else {
                parsed.args.push(arg.clone());
            }
            index += 1;
        } else if let Some(cwd) = arg.strip_prefix("--cwd=") {
            parsed.cwd = Some(PathBuf::from(cwd));
            index += 1;
        } else if arg == "--cwd" {
            if let Some(cwd) = args.get(index + 1) {
                parsed.cwd = Some(PathBuf::from(cwd));
                index += 2;
            } else {
                parsed.args.push(arg.clone());
                index += 1;
            }
        } else if arg == "-e" || arg == "--env" {
            if let Some(env) = args.get(index + 1) {
                if let Some((key, value)) = parse_key_value_arg(env) {
                    parsed.env.insert(key, interpolate_env(&value));
                } else {
                    parsed.args.push(arg.clone());
                    parsed.args.push(env.clone());
                }
                index += 2;
            } else {
                parsed.args.push(arg.clone());
                index += 1;
            }
        } else if let Some(env) = arg.strip_prefix("-e=").or_else(|| arg.strip_prefix("--env=")) {
            if let Some((key, value)) = parse_key_value_arg(env) {
                parsed.env.insert(key, interpolate_env(&value));
            } else {
                parsed.args.push(arg.clone());
            }
            index += 1;
        } else if arg == "-t" || arg == "--timeout" {
            if let Some(timeout) = args.get(index + 1) {
                if let Ok(seconds) = timeout.parse::<f64>() {
                    if seconds.is_finite() && seconds > 0.0 {
                        parsed.timeout = Some(Duration::from_secs_f64(seconds));
                    } else {
                        parsed.args.push(arg.clone());
                        parsed.args.push(timeout.clone());
                    }
                } else {
                    parsed.args.push(arg.clone());
                    parsed.args.push(timeout.clone());
                }
                index += 2;
            } else {
                parsed.args.push(arg.clone());
                index += 1;
            }
        } else if let Some(timeout) = arg
            .strip_prefix("-t=")
            .or_else(|| arg.strip_prefix("--timeout="))
        {
            if let Ok(seconds) = timeout.parse::<f64>() {
                if seconds.is_finite() && seconds > 0.0 {
                    parsed.timeout = Some(Duration::from_secs_f64(seconds));
                } else {
                    parsed.args.push(arg.clone());
                }
            } else {
                parsed.args.push(arg.clone());
            }
            index += 1;
        } else if let Some(mode) = arg.strip_prefix("--auth=") {
            match mode {
                "explicit-headers" | "headers" | "none" => {
                    parsed.auth_mode = BackendAuthMode::ExplicitHeaders;
                }
                "oauth" => {
                    parsed.auth_mode = BackendAuthMode::OAuth;
                }
                _ => parsed.args.push(arg.clone()),
            }
            index += 1;
        } else if arg == "--auth" {
            if let Some(mode) = args.get(index + 1) {
                match mode.as_str() {
                    "explicit-headers" | "headers" | "none" => {
                        parsed.auth_mode = BackendAuthMode::ExplicitHeaders;
                    }
                    "oauth" => {
                        parsed.auth_mode = BackendAuthMode::OAuth;
                    }
                    _ => {
                        parsed.args.push(arg.clone());
                        parsed.args.push(mode.clone());
                    }
                }
                index += 2;
            } else {
                parsed.args.push(arg.clone());
                index += 1;
            }
        } else {
            parsed.args.push(arg.clone());
            index += 1;
        }
    }
    parsed
}

fn parse_header_arg(header: &str) -> Option<(String, String)> {
    let (name, value) = header.split_once('=').or_else(|| header.split_once(':'))?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name.to_string(), interpolate_env(value)))
}

fn parse_key_value_arg(value: &str) -> Option<(String, String)> {
    let (key, value) = value.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

fn interpolate_env(value: &str) -> String {
    let mut output = String::new();
    let chars = value.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$' && chars.get(index + 1) == Some(&'{') {
            if let Some(end) = chars[index + 2..].iter().position(|ch| *ch == '}') {
                let name = chars[index + 2..index + 2 + end].iter().collect::<String>();
                output.push_str(&std::env::var(&name).unwrap_or_else(|_| format!("${{{name}}}")));
                index += end + 3;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_backend_url_parses_curl_style_headers_after_separator() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token", "--header", "X-Test=yes"],
        );

        assert_eq!(backend.transport, BackendTransport::StreamableHttp);
        assert!(backend.args.is_empty());
        assert_eq!(backend.headers["Authorization"], "Basic token");
        assert_eq!(backend.headers["X-Test"], "yes");
    }

    #[test]
    fn http_backend_url_parses_equals_header_forms() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H=Authorization=Bearer token", "--header=X-Test=yes"],
        );

        assert!(backend.args.is_empty());
        assert_eq!(backend.headers["Authorization"], "Bearer token");
        assert_eq!(backend.headers["X-Test"], "yes");
    }

    #[test]
    fn http_backend_header_values_preserve_missing_environment_variables() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            [
                "-H",
                "Authorization=Bearer ${MCP_COMPRESSOR_MISSING_TEST_TOKEN}",
            ],
        );

        assert_eq!(
            backend.headers["Authorization"],
            "Bearer ${MCP_COMPRESSOR_MISSING_TEST_TOKEN}"
        );
    }

    #[test]
    fn remote_http_auto_auth_uses_oauth_without_authorization_header() {
        let backend =
            BackendServerConfig::new("remote", "https://example.test/mcp", [] as [&str; 0]);

        assert!(backend.should_use_oauth());
    }

    #[test]
    fn remote_http_auto_auth_skips_oauth_with_authorization_header() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token"],
        );

        assert!(backend.has_authorization_header());
        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn http_backend_url_parses_auth_mode_args() {
        let explicit = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["--auth", "explicit-headers"],
        );
        let oauth =
            BackendServerConfig::new("remote", "https://example.test/mcp", ["--auth=oauth"]);

        assert_eq!(explicit.auth_mode, BackendAuthMode::ExplicitHeaders);
        assert!(explicit.args.is_empty());
        assert_eq!(oauth.auth_mode, BackendAuthMode::OAuth);
        assert!(oauth.args.is_empty());
    }

    #[test]
    fn explicit_headers_auth_mode_skips_oauth_without_authorization_header() {
        let backend =
            BackendServerConfig::new("remote", "https://example.test/mcp", [] as [&str; 0])
                .with_auth_mode(BackendAuthMode::ExplicitHeaders);

        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn forced_oauth_auth_mode_uses_oauth_even_with_authorization_header() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["-H", "Authorization=Basic token"],
        )
        .with_auth_mode(BackendAuthMode::OAuth);

        assert!(backend.should_use_oauth());
    }

    #[test]
    fn stdio_backend_never_uses_oauth() {
        let backend = BackendServerConfig::new("local", "python", ["server.py"]);

        assert!(!backend.should_use_oauth());
    }

    #[test]
    fn backend_args_parse_cwd_env_and_timeout_after_separator() {
        let backend = BackendServerConfig::new(
            "local",
            "python",
            [
                "server.py",
                "--cwd",
                "/tmp/example",
                "-e",
                "FOO=bar",
                "--env=EMPTY=",
                "-t",
                "2.5",
            ],
        );

        assert_eq!(backend.args, ["server.py"]);
        assert_eq!(backend.cwd.as_deref(), Some(std::path::Path::new("/tmp/example")));
        assert_eq!(backend.env["FOO"], "bar");
        assert_eq!(backend.env["EMPTY"], "");
        assert_eq!(backend.timeout, Some(Duration::from_secs_f64(2.5)));
    }

    #[test]
    fn backend_args_preserve_invalid_timeout_for_backend_validation() {
        let backend = BackendServerConfig::new("local", "python", ["server.py", "--timeout", "0"]);

        assert_eq!(backend.args, ["server.py", "--timeout", "0"]);
        assert_eq!(backend.timeout, None);
    }

    #[test]
    fn http_backend_url_preserves_unrecognized_args_for_validation() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["--unknown", "value", "-H"],
        );

        assert_eq!(backend.args, ["--unknown", "value", "-H"]);
        assert!(backend.headers.is_empty());
    }
}
