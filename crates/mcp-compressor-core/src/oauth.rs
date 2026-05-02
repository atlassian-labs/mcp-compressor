//! OAuth helpers for remote MCP backends.
//!
//! The runtime delegates OAuth protocol details to `rmcp`. This module only
//! provides compressor-specific storage and local callback plumbing.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::transport::auth::{
    AuthError, CredentialStore, StateStore, StoredAuthorizationState, StoredCredentials,
};

/// File-backed OAuth credential store.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait::async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let Some(contents) = read_optional(&self.path)? else {
            return Ok(None);
        };
        serde_json::from_str(&contents).map(Some).map_err(|error| {
            AuthError::InternalError(format!("failed to parse OAuth credentials: {error}"))
        })
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        write_json(&self.path, &credentials)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        remove_optional(&self.path)
    }
}

/// File-backed OAuth authorization-state store.
#[derive(Debug, Clone)]
pub struct FileStateStore {
    dir: PathBuf,
}

impl FileStateStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn state_path(&self, csrf_token: &str) -> PathBuf {
        self.dir
            .join(format!("{}.json", sanitize_file_component(csrf_token)))
    }
}

#[async_trait::async_trait]
impl StateStore for FileStateStore {
    async fn save(
        &self,
        csrf_token: &str,
        state: StoredAuthorizationState,
    ) -> Result<(), AuthError> {
        write_json(&self.state_path(csrf_token), &state)
    }

    async fn load(&self, csrf_token: &str) -> Result<Option<StoredAuthorizationState>, AuthError> {
        let Some(contents) = read_optional(&self.state_path(csrf_token))? else {
            return Ok(None);
        };
        serde_json::from_str(&contents).map(Some).map_err(|error| {
            AuthError::InternalError(format!("failed to parse OAuth state: {error}"))
        })
    }

    async fn delete(&self, csrf_token: &str) -> Result<(), AuthError> {
        remove_optional(&self.state_path(csrf_token))
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, AuthError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AuthError::InternalError(format!(
            "failed to read OAuth store {}: {error}",
            path.display()
        ))),
    }
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), AuthError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AuthError::InternalError(format!(
                "failed to create OAuth store directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|error| {
        AuthError::InternalError(format!("failed to serialize OAuth store: {error}"))
    })?;
    fs::write(path, json).map_err(|error| {
        AuthError::InternalError(format!(
            "failed to write OAuth store {}: {error}",
            path.display()
        ))
    })
}

fn remove_optional(path: &Path) -> Result<(), AuthError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AuthError::InternalError(format!(
            "failed to remove OAuth store {}: {error}",
            path.display()
        ))),
    }
}

/// Local OAuth callback listener bound to loopback.
#[derive(Debug)]
pub struct OAuthCallbackListener {
    listener: TcpListener,
    redirect_uri: String,
}

impl OAuthCallbackListener {
    pub fn bind() -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        Ok(Self {
            listener,
            redirect_uri: format!("http://{addr}/callback"),
        })
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn local_addr(&self) -> Result<SocketAddr, std::io::Error> {
        self.listener.local_addr()
    }

    pub fn wait_for_callback(self) -> Result<OAuthCallback, std::io::Error> {
        let (mut stream, _) = self.listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut request = [0_u8; 8192];
        let bytes = stream.read(&mut request)?;
        let request = String::from_utf8_lossy(&request[..bytes]);
        match parse_callback_request(&request) {
            OAuthCallbackResult::Success(callback) => {
                write_callback_response(
                    &mut stream,
                    200,
                    "OAuth complete. You can close this tab and return to mcp-compressor.",
                )?;
                Ok(callback)
            }
            OAuthCallbackResult::ProviderError { error, description } => {
                write_callback_response(
                    &mut stream,
                    400,
                    "OAuth authorization failed. You can close this tab and return to mcp-compressor.",
                )?;
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format_callback_provider_error(&error, description.as_deref()),
                ))
            }
            OAuthCallbackResult::Malformed(reason) => {
                write_callback_response(
                    &mut stream,
                    400,
                    "OAuth callback was missing required parameters. You can close this tab.",
                )?;
                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCallback {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OAuthCallbackResult {
    Success(OAuthCallback),
    ProviderError {
        error: String,
        description: Option<String>,
    },
    Malformed(String),
}

fn parse_callback_request(request: &str) -> OAuthCallbackResult {
    let Some(first_line) = request.lines().next() else {
        return OAuthCallbackResult::Malformed("OAuth callback request was empty".to_string());
    };
    let Some(path) = first_line.split_whitespace().nth(1) else {
        return OAuthCallbackResult::Malformed(
            "OAuth callback request line was invalid".to_string(),
        );
    };
    let Some(query) = path.split_once('?').map(|(_, query)| query) else {
        return OAuthCallbackResult::Malformed(
            "OAuth callback query string was missing".to_string(),
        );
    };
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "code" => code = Some(percent_decode(value)),
            "state" => state = Some(percent_decode(value)),
            "error" => error = Some(percent_decode(value)),
            "error_description" => error_description = Some(percent_decode(value)),
            _ => {}
        }
    }
    if let Some(error) = error {
        return OAuthCallbackResult::ProviderError {
            error,
            description: error_description,
        };
    }
    match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() && !state.is_empty() => {
            OAuthCallbackResult::Success(OAuthCallback { code, state })
        }
        _ => OAuthCallbackResult::Malformed(
            "OAuth callback was missing non-empty code or state".to_string(),
        ),
    }
}

fn write_callback_response(
    stream: &mut impl Write,
    status: u16,
    body: &str,
) -> Result<(), std::io::Error> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())
}

fn format_callback_provider_error(error: &str, description: Option<&str>) -> String {
    match description {
        Some(description) if !description.is_empty() => {
            format!("OAuth provider returned {error}: {description}")
        }
        _ => format!("OAuth provider returned {error}"),
    }
}

fn percent_decode(value: &str) -> String {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    output.push((high << 4) | low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "state".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_credential_store_missing_loads_none_and_clear_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileCredentialStore::new(dir.path().join("credentials.json"));

        assert!(store.load().await.unwrap().is_none());
        store.clear().await.unwrap();
    }

    #[tokio::test]
    async fn file_state_store_missing_loads_none_and_delete_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStateStore::new(dir.path().join("state"));

        assert!(store.load("missing-token").await.unwrap().is_none());
        store.delete("missing-token").await.unwrap();
    }

    #[test]
    fn callback_request_parser_extracts_and_decodes_code_and_state() {
        let callback = parse_callback_request(
            "GET /callback?code=abc%20123&state=state+value HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );

        assert_eq!(
            callback,
            OAuthCallbackResult::Success(OAuthCallback {
                code: "abc 123".to_string(),
                state: "state value".to_string(),
            })
        );
    }

    #[test]
    fn callback_request_parser_reports_provider_errors() {
        let callback = parse_callback_request(
            "GET /callback?error=access_denied&error_description=user+cancelled HTTP/1.1\r\n\r\n",
        );

        assert_eq!(
            callback,
            OAuthCallbackResult::ProviderError {
                error: "access_denied".to_string(),
                description: Some("user cancelled".to_string()),
            }
        );
    }

    #[test]
    fn callback_request_parser_rejects_missing_fields() {
        assert!(matches!(
            parse_callback_request("GET /callback?code=abc HTTP/1.1\r\n\r\n"),
            OAuthCallbackResult::Malformed(_)
        ));
        assert!(matches!(
            parse_callback_request("GET /callback?state=abc HTTP/1.1\r\n\r\n"),
            OAuthCallbackResult::Malformed(_)
        ));
    }

    #[test]
    fn callback_response_writes_status_and_body() {
        let mut response = Vec::new();
        write_callback_response(&mut response, 400, "nope").unwrap();
        let response = String::from_utf8(response).unwrap();

        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(response.contains("Content-Length: 4"));
        assert!(response.ends_with("\r\n\r\nnope"));
    }

    #[test]
    fn state_store_sanitizes_file_components() {
        let store = FileStateStore::new("state-dir");

        assert_eq!(
            store.state_path("abc/../def").file_name().unwrap(),
            "abc____def.json"
        );
        assert_eq!(store.state_path("").file_name().unwrap(), "state.json");
    }
}
