use std::collections::HashMap;
use std::str::FromStr;

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

/// Configuration for one upstream MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
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
        let (args, headers, auth_mode) = if transport == BackendTransport::StreamableHttp {
            parse_http_backend_args(raw_args)
        } else {
            (raw_args, HashMap::new(), BackendAuthMode::Auto)
        };
        Self {
            name: name.into(),
            command,
            args,
            env: HashMap::new(),
            transport,
            headers,
            auth_mode,
        }
    }

    pub fn with_env(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env = env.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
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

fn parse_http_backend_args(
    args: Vec<String>,
) -> (Vec<String>, HashMap<String, String>, BackendAuthMode) {
    let mut remaining = Vec::new();
    let mut headers = HashMap::new();
    let mut auth_mode = BackendAuthMode::Auto;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-H" || arg == "--header" {
            if let Some(header) = args.get(index + 1) {
                if let Some((name, value)) = parse_header_arg(header) {
                    headers.insert(name, value);
                } else {
                    remaining.push(arg.clone());
                    remaining.push(header.clone());
                }
                index += 2;
            } else {
                remaining.push(arg.clone());
                index += 1;
            }
        } else if let Some(mode) = arg.strip_prefix("--auth=") {
            match mode {
                "explicit-headers" | "headers" | "none" => {
                    auth_mode = BackendAuthMode::ExplicitHeaders;
                }
                "oauth" => {
                    auth_mode = BackendAuthMode::OAuth;
                }
                _ => remaining.push(arg.clone()),
            }
            index += 1;
        } else if arg == "--auth" {
            if let Some(mode) = args.get(index + 1) {
                match mode.as_str() {
                    "explicit-headers" | "headers" | "none" => {
                        auth_mode = BackendAuthMode::ExplicitHeaders;
                    }
                    "oauth" => {
                        auth_mode = BackendAuthMode::OAuth;
                    }
                    _ => {
                        remaining.push(arg.clone());
                        remaining.push(mode.clone());
                    }
                }
                index += 2;
            } else {
                remaining.push(arg.clone());
                index += 1;
            }
        } else if let Some(header) = arg
            .strip_prefix("-H=")
            .or_else(|| arg.strip_prefix("--header="))
        {
            if let Some((name, value)) = parse_header_arg(header) {
                headers.insert(name, value);
            } else {
                remaining.push(arg.clone());
            }
            index += 1;
        } else {
            remaining.push(arg.clone());
            index += 1;
        }
    }
    (remaining, headers, auth_mode)
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
    fn http_backend_url_preserves_unrecognized_args_for_validation() {
        let backend = BackendServerConfig::new(
            "remote",
            "https://example.test/mcp",
            ["--timeout", "30", "-H"],
        );

        assert_eq!(backend.args, ["--timeout", "30", "-H"]);
        assert!(backend.headers.is_empty());
    }
}
