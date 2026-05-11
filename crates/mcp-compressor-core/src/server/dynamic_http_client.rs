use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue};
use futures::stream::BoxStream;
use futures::StreamExt;
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::streamable_http_client::SseError;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Sse, SseStream};

const HEADER_SESSION_ID: &str = "mcp-session-id";
const HEADER_LAST_EVENT_ID: &str = "last-event-id";

use crate::server::backend::HeaderProvider;

const EVENT_STREAM_MIME_TYPE: &str = "text/event-stream";
const JSON_MIME_TYPE: &str = "application/json";

#[derive(Clone)]
pub(crate) struct DynamicAuthHttpClient {
    client: reqwest::Client,
    static_headers: HashMap<HeaderName, HeaderValue>,
    provider: HeaderProvider,
}

impl DynamicAuthHttpClient {
    pub(crate) fn new(
        client: reqwest::Client,
        static_headers: HashMap<HeaderName, HeaderValue>,
        provider: HeaderProvider,
    ) -> Self {
        Self {
            client,
            static_headers,
            provider,
        }
    }

    fn merged_headers(&self) -> Result<HashMap<HeaderName, HeaderValue>, DynamicAuthHttpError> {
        let mut headers = self.static_headers.clone();
        let dynamic = (self.provider)().map_err(|error| DynamicAuthHttpError(error.to_string()))?;
        for (name, value) in dynamic {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                DynamicAuthHttpError(format!("invalid HTTP header name {name:?}: {error}"))
            })?;
            let value = HeaderValue::from_str(&value).map_err(|error| {
                DynamicAuthHttpError(format!("invalid HTTP header value for {name:?}: {error}"))
            })?;
            headers.insert(name, value);
        }
        Ok(headers)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("dynamic auth HTTP client error: {0}")]
pub(crate) struct DynamicAuthHttpError(String);

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: HashMap<HeaderName, HeaderValue>,
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

fn parse_json_rpc_error(body: &str) -> Option<ServerJsonRpcMessage> {
    serde_json::from_str(body).ok()
}

impl From<DynamicAuthHttpError> for StreamableHttpError<DynamicAuthHttpError> {
    fn from(value: DynamicAuthHttpError) -> Self {
        Self::Client(value)
    }
}

impl DynamicAuthHttpClient {
    #[cfg(test)]
    pub(crate) async fn post_for_test(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<DynamicAuthHttpError>> {
        self.post_message(uri, message, None, None, HashMap::new())
            .await
    }
}

impl StreamableHttpClient for DynamicAuthHttpClient {
    type Error = DynamicAuthHttpError;

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let mut headers = self.merged_headers()?;
        headers.extend(custom_headers);
        let mut request = self
            .client
            .get(uri.as_ref())
            .header(
                reqwest::header::ACCEPT,
                [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "),
            )
            .header(HEADER_SESSION_ID, session_id.as_ref());
        if let Some(last_event_id) = last_event_id {
            request = request.header(HEADER_LAST_EVENT_ID, last_event_id);
        }
        if let Some(auth_header) = auth_token {
            request = request.bearer_auth(auth_header);
        }
        let response = apply_headers(request, headers)
            .send()
            .await
            .map_err(|error| {
                StreamableHttpError::Client(DynamicAuthHttpError(error.to_string()))
            })?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        let response = response.error_for_status().map_err(|error| {
            StreamableHttpError::Client(DynamicAuthHttpError(error.to_string()))
        })?;
        match response.headers().get(reqwest::header::CONTENT_TYPE) {
            Some(ct)
                if ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes())
                    || ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {}
            Some(ct) => {
                return Err(StreamableHttpError::UnexpectedContentType(Some(
                    String::from_utf8_lossy(ct.as_bytes()).to_string(),
                )))
            }
            None => return Err(StreamableHttpError::UnexpectedContentType(None)),
        }
        Ok(SseStream::from_byte_stream(response.bytes_stream()).boxed())
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let mut headers = self.merged_headers()?;
        headers.extend(custom_headers);
        let mut request = self
            .client
            .delete(uri.as_ref())
            .header(HEADER_SESSION_ID, session.as_ref());
        if let Some(auth_header) = auth_token {
            request = request.bearer_auth(auth_header);
        }
        let response = apply_headers(request, headers)
            .send()
            .await
            .map_err(|error| {
                StreamableHttpError::Client(DynamicAuthHttpError(error.to_string()))
            })?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        response.error_for_status().map_err(|error| {
            StreamableHttpError::Client(DynamicAuthHttpError(error.to_string()))
        })?;
        Ok(())
    }

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_token: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let mut headers = self.merged_headers()?;
        headers.extend(custom_headers);
        let mut request = self.client.post(uri.as_ref()).header(
            reqwest::header::ACCEPT,
            [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "),
        );
        if let Some(auth_header) = auth_token {
            request = request.bearer_auth(auth_header);
        }
        let session_was_attached = session_id.is_some();
        if let Some(session_id) = session_id {
            request = request.header(HEADER_SESSION_ID, session_id.as_ref());
        }
        let response = apply_headers(request, headers)
            .json(&message)
            .send()
            .await
            .map_err(|error| {
                StreamableHttpError::Client(DynamicAuthHttpError(error.to_string()))
            })?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Some(header) = response.headers().get(reqwest::header::WWW_AUTHENTICATE) {
                let header = header
                    .to_str()
                    .map_err(|_| {
                        StreamableHttpError::UnexpectedServerResponse(Cow::from(
                            "invalid www-authenticate header value",
                        ))
                    })?
                    .to_string();
                return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                    format!("auth required: {header}"),
                )));
            }
        }
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            if let Some(header) = response.headers().get(reqwest::header::WWW_AUTHENTICATE) {
                let header_str = header.to_str().map_err(|_| {
                    StreamableHttpError::UnexpectedServerResponse(Cow::from(
                        "invalid www-authenticate header value",
                    ))
                })?;
                return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                    format!("insufficient scope: {header_str}"),
                )));
            }
        }

        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == reqwest::StatusCode::NOT_FOUND && session_was_attached {
            return Err(StreamableHttpError::SessionExpired);
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string());
        let session_id = response
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_owned());
            if content_type
                .as_deref()
                .is_some_and(|ct| ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()))
            {
                if let Some(message) = parse_json_rpc_error(&body) {
                    return Ok(StreamableHttpPostResponse::Json(message, session_id));
                }
            }
            return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
                format!("HTTP {status}: {body}"),
            )));
        }
        match content_type.as_deref() {
            Some(ct) if ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) => {
                Ok(StreamableHttpPostResponse::Sse(
                    SseStream::from_byte_stream(response.bytes_stream()).boxed(),
                    session_id,
                ))
            }
            Some(ct) if ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {
                match response.json::<ServerJsonRpcMessage>().await {
                    Ok(message) => Ok(StreamableHttpPostResponse::Json(message, session_id)),
                    Err(_) => Ok(StreamableHttpPostResponse::Accepted),
                }
            }
            _ => Err(StreamableHttpError::UnexpectedContentType(content_type)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use rmcp::model::{
        ClientJsonRpcMessage, EmptyResult, JsonRpcRequest, RequestId, ServerJsonRpcMessage,
        ServerResult,
    };
    use rmcp::transport::streamable_http_client::StreamableHttpPostResponse;
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone)]
    struct AuthState {
        expected: Arc<AtomicUsize>,
    }

    async fn auth_handler(
        State(state): State<AuthState>,
        headers: HeaderMap,
        Json(_body): Json<serde_json::Value>,
    ) -> Result<Json<ServerJsonRpcMessage>, StatusCode> {
        let expected = state.expected.fetch_add(1, Ordering::SeqCst) + 1;
        let expected_header = format!("Bearer token-{expected}");
        let actual = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if actual != expected_header {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(Json(ServerJsonRpcMessage::Response(
            rmcp::model::JsonRpcResponse {
                jsonrpc: rmcp::model::JsonRpcVersion2_0,
                id: RequestId::Number(expected as i64),
                result: ServerResult::EmptyResult(EmptyResult {}),
            },
        )))
    }

    fn test_message(id: i64) -> ClientJsonRpcMessage {
        ClientJsonRpcMessage::Request(JsonRpcRequest {
            jsonrpc: rmcp::model::JsonRpcVersion2_0,
            id: RequestId::Number(id),
            request: rmcp::model::ClientRequest::PingRequest(Default::default()),
        })
    }

    #[tokio::test]
    async fn dynamic_provider_is_called_for_each_post_request() {
        let state = AuthState {
            expected: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/mcp", post(auth_handler))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let calls = Arc::new(AtomicUsize::new(0));
        let provider_calls = Arc::clone(&calls);
        let client = DynamicAuthHttpClient::new(
            reqwest::Client::new(),
            HashMap::new(),
            Arc::new(move || {
                let call = provider_calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(BTreeMap::from([(
                    "Authorization".to_string(),
                    format!("Bearer token-{call}"),
                )]))
            }),
        );
        let uri = Arc::<str>::from(format!("http://{addr}/mcp"));

        let first = client
            .post_for_test(Arc::clone(&uri), test_message(1))
            .await
            .unwrap();
        let second = client.post_for_test(uri, test_message(2)).await.unwrap();

        assert!(matches!(first, StreamableHttpPostResponse::Json(_, _)));
        assert!(matches!(second, StreamableHttpPostResponse::Json(_, _)));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(state.expected.load(Ordering::SeqCst), 2);
    }
}
