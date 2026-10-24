//! Observe HTTP dispatches without replacing rmcp's transport or response handling.

use super::DATA_SOURCE_IFIND;
use crate::RequestRecorder;
use chrono::Utc;
use futures::stream::BoxStream;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::streamable_http_client::{
        StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
    },
};
use sse_stream::{Error as SseError, Sse};
use std::{collections::HashMap, sync::Arc};

type TransportError = StreamableHttpError<reqwest::Error>;

#[derive(Clone)]
pub(super) struct RecordedHttpClient {
    client: reqwest::Client,
    recorder: Arc<dyn RequestRecorder>,
}

impl RecordedHttpClient {
    pub(super) fn new(client: reqwest::Client, recorder: Arc<dyn RequestRecorder>) -> Self {
        Self { client, recorder }
    }

    // rmcp 2.1's reqwest adapter performs exactly one send per trait method, but
    // does not expose its built request. Preflight the same dynamic inputs so
    // local builder/header errors do not count. The SDK still owns the actual
    // send, authentication, response parsing, SSE lifecycle and retry policy.
    fn preflight(
        &self,
        method: reqwest::Method,
        uri: &str,
        session_id: Option<&str>,
        auth_header: Option<&str>,
        custom_headers: &HashMap<HeaderName, HeaderValue>,
    ) -> Result<reqwest::RequestBuilder, TransportError> {
        let mut request = self.client.request(method, uri);
        if let Some(auth_header) = auth_header {
            request = request.bearer_auth(auth_header);
        }
        for (name, value) in custom_headers {
            // Match rmcp's reserved-header validation. MCP-Protocol-Version is
            // allowed because the SDK injects it after initialization.
            if matches!(name.as_str(), "accept" | "mcp-session-id" | "last-event-id") {
                return Err(StreamableHttpError::ReservedHeaderConflict(
                    name.to_string(),
                ));
            }
            request = request.header(name, value);
        }
        if let Some(session_id) = session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }
        Ok(request)
    }

    async fn record(&self, request: reqwest::RequestBuilder) -> Result<(), TransportError> {
        request.build().map_err(StreamableHttpError::Client)?;
        self.recorder
            .record_request(DATA_SOURCE_IFIND, Utc::now())
            .await;
        Ok(())
    }
}

impl StreamableHttpClient for RecordedHttpClient {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, TransportError> {
        let request = self.preflight(
            reqwest::Method::POST,
            &uri,
            session_id.as_deref(),
            auth_header.as_deref(),
            &custom_headers,
        )?;
        self.record(request.json(&message)).await?;
        self.client
            .post_message(uri, message, session_id, auth_header, custom_headers)
            .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), TransportError> {
        let request = self.preflight(
            reqwest::Method::DELETE,
            &uri,
            Some(&session_id),
            auth_header.as_deref(),
            &custom_headers,
        )?;
        self.record(request).await?;
        self.client
            .delete_session(uri, session_id, auth_header, custom_headers)
            .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, TransportError> {
        let mut request = self.preflight(
            reqwest::Method::GET,
            &uri,
            Some(&session_id),
            auth_header.as_deref(),
            &custom_headers,
        )?;
        if let Some(last_event_id) = &last_event_id {
            request = request.header("Last-Event-Id", last_event_id);
        }
        self.record(request).await?;
        self.client
            .get_stream(uri, session_id, last_event_id, auth_header, custom_headers)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_recorder::tests::{MemoryRecorder, MockServer};
    use rmcp::model::{ClientRequest, PingRequest, RequestId};
    use std::time::Duration;

    fn ping() -> ClientJsonRpcMessage {
        ClientJsonRpcMessage::request(
            ClientRequest::PingRequest(PingRequest::default()),
            RequestId::Number(1),
        )
    }

    #[tokio::test]
    async fn local_request_errors_do_not_count_or_connect() {
        let recorder = Arc::new(MemoryRecorder::default());
        let client = RecordedHttpClient::new(wealthfolio_http::client(), recorder.clone());
        let uri: Arc<str> = "http://127.0.0.1:9".into();
        assert!(client
            .post_message("not a URL".into(), ping(), None, None, HashMap::new())
            .await
            .is_err());
        assert!(client
            .get_stream(
                "not a URL".into(),
                "session".into(),
                None,
                None,
                HashMap::new()
            )
            .await
            .is_err());
        assert!(client
            .delete_session("not a URL".into(), "session".into(), None, HashMap::new())
            .await
            .is_err());
        for header in ["accept", "mcp-session-id", "last-event-id"] {
            assert!(matches!(
                client
                    .post_message(
                        uri.clone(),
                        ping(),
                        None,
                        None,
                        HashMap::from([(
                            HeaderName::from_static(header),
                            HeaderValue::from_static("invalid")
                        )]),
                    )
                    .await,
                Err(StreamableHttpError::ReservedHeaderConflict(_))
            ));
        }
        assert!(client
            .post_message(
                uri.clone(),
                ping(),
                Some("bad\nvalue".into()),
                None,
                HashMap::new()
            )
            .await
            .is_err());
        assert!(client
            .post_message(
                uri.clone(),
                ping(),
                None,
                Some("bad\nvalue".into()),
                HashMap::new()
            )
            .await
            .is_err());
        assert!(client
            .get_stream(
                uri.clone(),
                "session".into(),
                Some("bad\nvalue".into()),
                None,
                HashMap::new()
            )
            .await
            .is_err());
        assert!(client
            .delete_session(uri, "bad\nvalue".into(), None, HashMap::new())
            .await
            .is_err());
        assert!(recorder.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn http_errors_and_connection_failures_count_each_transport_dispatch_once() {
        let recorder = Arc::new(MemoryRecorder::default());
        let client = RecordedHttpClient::new(wealthfolio_http::client(), recorder.clone());
        let server = MockServer::start(vec![
            (500, String::new(), String::new()),
            (429, String::new(), String::new()),
            (403, String::new(), String::new()),
        ]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let refused = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        for uri in [&server.url, &refused] {
            assert!(client
                .post_message(uri.as_str().into(), ping(), None, None, HashMap::new())
                .await
                .is_err());
            assert!(client
                .get_stream(
                    uri.as_str().into(),
                    "session".into(),
                    None,
                    None,
                    HashMap::new()
                )
                .await
                .is_err());
            assert!(client
                .delete_session(uri.as_str().into(), "session".into(), None, HashMap::new())
                .await
                .is_err());
        }
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 6);
        assert!(events.iter().all(|(id, _)| id == DATA_SOURCE_IFIND));
    }

    #[tokio::test]
    async fn timeout_counts_before_response_without_replay() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let recorder = Arc::new(MemoryRecorder::default());
        let client = RecordedHttpClient::new(
            wealthfolio_http::client_builder()
                .timeout(Duration::from_millis(50))
                .build()
                .unwrap(),
            recorder.clone(),
        );
        assert!(matches!(
            client.post_message(uri.into(), ping(), None, None, HashMap::new()).await,
            Err(StreamableHttpError::Client(error)) if error.is_timeout()
        ));
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
        server.abort();
    }
}
