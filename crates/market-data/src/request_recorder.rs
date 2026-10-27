//! Profile-owned observation of downstream API request dispatches.
//!
//! Providers supply explicitly selected business parameters, never credentials.
//! Implementations must fail open: an unavailable statistics store must not change the request
//! result or cause the caller to retry a network operation.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    pub record_kind: String,
    pub operation: String,
    pub symbols: Vec<String>,
    pub parameters: Value,
}

impl RequestContext {
    pub fn http(operation: &str, symbols: Vec<String>, parameters: Value) -> Self {
        Self {
            record_kind: "http".into(),
            operation: operation.into(),
            symbols,
            parameters,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOutcome {
    pub duration_ms: u64,
    pub outcome: String,
    pub http_status: Option<u16>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
}

impl RequestOutcome {
    pub fn completed(
        started: Instant,
        error: Option<(&str, &str)>,
        http_status: Option<u16>,
    ) -> Self {
        Self {
            duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            outcome: if error.is_some() {
                "failure"
            } else {
                "success"
            }
            .into(),
            http_status,
            error_category: error.map(|(category, _)| category.into()),
            error_message: error.map(|(_, message)| message.into()),
        }
    }
}

#[async_trait]
pub trait RequestRecorder: Send + Sync {
    async fn record_request(&self, provider_id: &str, dispatched_at: DateTime<Utc>);

    async fn start_request(
        &self,
        provider_id: &str,
        at: DateTime<Utc>,
        _context: &RequestContext,
    ) -> Option<i64> {
        self.record_request(provider_id, at).await;
        None
    }

    async fn finish_request(&self, _id: i64, _outcome: &RequestOutcome) {}
}

/// Default for standalone providers and tests that do not persist statistics.
#[derive(Debug, Default)]
pub struct NoopRequestRecorder;

#[async_trait]
impl RequestRecorder for NoopRequestRecorder {
    async fn record_request(&self, _provider_id: &str, _dispatched_at: DateTime<Utc>) {}
}

/// Count an application API dispatch, not reqwest's automatic redirects or
/// protocol retries. Building the request first excludes local builder errors.
#[async_trait]
pub trait RecordedRequest {
    async fn send_recorded(
        self,
        provider_id: &str,
        recorder: &dyn RequestRecorder,
        context: &RequestContext,
    ) -> Result<reqwest::Response, reqwest::Error>;
}

#[async_trait]
impl RecordedRequest for reqwest::RequestBuilder {
    async fn send_recorded(
        self,
        provider_id: &str,
        recorder: &dyn RequestRecorder,
        context: &RequestContext,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let (client, request) = self.build_split();
        let request = request?;
        (client, request)
            .send_recorded(provider_id, recorder, context)
            .await
    }
}

#[async_trait]
impl RecordedRequest for (reqwest::Client, reqwest::Request) {
    async fn send_recorded(
        self,
        provider_id: &str,
        recorder: &dyn RequestRecorder,
        context: &RequestContext,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let (client, request) = self;
        let id = recorder
            .start_request(provider_id, Utc::now(), context)
            .await;
        let started = Instant::now();
        let result = client.execute(request).await;
        let status = result
            .as_ref()
            .ok()
            .map(|response| response.status().as_u16());
        let error = match &result {
            Ok(response) if !response.status().is_success() => {
                Some(("http_status", "HTTP non-success status"))
            }
            Err(error) if error.is_timeout() => Some(("timeout", "Request timed out")),
            Err(error) if error.is_connect() => Some(("connection", "Connection failed")),
            Err(_) => Some(("network", "Network request failed")),
            _ => None,
        };
        let outcome = RequestOutcome::completed(started, error, status);
        if let Some(id) = id {
            recorder.finish_request(id, &outcome).await;
        }
        result
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    pub(crate) struct MemoryRecorder(
        pub Mutex<Vec<(String, DateTime<Utc>)>>,
        pub Mutex<Vec<RequestContext>>,
        pub Mutex<Vec<(i64, RequestOutcome)>>,
    );

    #[async_trait]
    impl RequestRecorder for MemoryRecorder {
        async fn start_request(
            &self,
            provider: &str,
            at: DateTime<Utc>,
            context: &RequestContext,
        ) -> Option<i64> {
            let mut events = self.0.lock().unwrap();
            events.push((provider.into(), at));
            let id = events.len() as i64;
            self.1.lock().unwrap().push(context.clone());
            Some(id)
        }

        async fn finish_request(&self, id: i64, outcome: &RequestOutcome) {
            self.2.lock().unwrap().push((id, outcome.clone()));
        }

        async fn record_request(&self, provider_id: &str, dispatched_at: DateTime<Utc>) {
            self.0
                .lock()
                .unwrap()
                .push((provider_id.into(), dispatched_at));
        }
    }

    pub(crate) fn offline_client() -> reqwest::Client {
        wealthfolio_http::client_builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:0").unwrap())
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap()
    }

    pub(crate) fn assert_safe_context(recorder: &MemoryRecorder, secret: &str) -> RequestContext {
        let contexts = recorder.1.lock().unwrap();
        assert_eq!(contexts.len(), 1);
        let context = contexts[0].clone();
        let json = serde_json::to_string(&context).unwrap();
        assert!(!json.contains(secret));
        assert!(!json.contains("unexpected_private_field"));
        context
    }

    #[tokio::test]
    async fn cancellation_keeps_unknown_and_transport_timeout_records_duration() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let client = wealthfolio_http::client_builder()
            .no_proxy()
            .build()
            .unwrap();
        let context = RequestContext::http(
            "latest",
            vec!["ABC".into()],
            serde_json::json!({"symbol":"ABC"}),
        );
        let recorder = MemoryRecorder::default();
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            client.get(&url).send_recorded("TEST", &recorder, &context)
        )
        .await
        .is_err());
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
        assert!(recorder.2.lock().unwrap().is_empty());
        assert!(client
            .get(url)
            .timeout(Duration::from_millis(50))
            .send_recorded("TEST", &recorder, &context)
            .await
            .is_err());
        let outcomes = recorder.2.lock().unwrap();
        assert_eq!(outcomes[0].0, 2);
        assert_eq!(outcomes[0].1.error_category.as_deref(), Some("timeout"));
        assert!(outcomes[0].1.duration_ms >= 40);
    }

    pub(crate) struct MockServer {
        pub url: String,
        pub requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl MockServer {
        pub fn start(responses: Vec<(u16, String, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let received = requests.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let thread = std::thread::spawn(move || {
                let mut responses = responses.into_iter();
                while !stopping.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                        Err(error) => panic!("mock server accept failed: {error}"),
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                            break;
                        }
                        request.push_str(&line);
                    }
                    received.lock().unwrap().push(request);
                    let (status, headers, body) =
                        responses.next().expect("unexpected HTTP request");
                    write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
                }
            });
            Self {
                url,
                requests,
                stop,
                thread: Some(thread),
            }
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    #[tokio::test]
    async fn dispatch_counts_status_failures_and_utc_timestamps() {
        let server = MockServer::start(vec![
            (200, String::new(), "ok".into()),
            (429, String::new(), String::new()),
        ]);
        let recorder = MemoryRecorder::default();
        let client = wealthfolio_http::client();
        let before = Utc::now();
        for expected_status in [200, 429] {
            let response = client
                .get(&server.url)
                .send_recorded(
                    "EASTMONEY",
                    &recorder,
                    &RequestContext::http("test", vec![], serde_json::json!({})),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected_status);
        }
        let after = Utc::now();
        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|(id, time)| id == "EASTMONEY" && *time >= before && *time <= after));
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let outcomes = recorder.2.lock().unwrap();
        assert_eq!(outcomes[0].1.outcome, "success");
        assert_eq!(outcomes[1].1.outcome, "failure");
        assert_eq!(outcomes[1].1.http_status, Some(429));
        assert_eq!(outcomes[1].1.error_category.as_deref(), Some("http_status"));
    }

    #[tokio::test]
    async fn local_builder_errors_do_not_count_but_connection_failures_do() {
        let recorder = MemoryRecorder::default();
        let client = wealthfolio_http::client();
        assert!(client
            .get("not a URL")
            .send_recorded(
                "SINA",
                &recorder,
                &RequestContext::http("test", vec![], serde_json::json!({}))
            )
            .await
            .is_err());
        assert!(recorder.0.lock().unwrap().is_empty());
        assert!(client
            .get("http://127.0.0.1:0")
            .send_recorded(
                "SINA",
                &recorder,
                &RequestContext::http("test", vec![], serde_json::json!({}))
            )
            .await
            .is_err());
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
        assert_eq!(recorder.2.lock().unwrap()[0].1.outcome, "failure");
    }

    #[tokio::test]
    async fn automatic_redirects_are_not_extra_application_dispatches() {
        let server = MockServer::start(vec![
            (302, "Location: /next\r\n".into(), String::new()),
            (200, String::new(), "ok".into()),
        ]);
        let recorder = MemoryRecorder::default();
        let response = wealthfolio_http::client()
            .get(&server.url)
            .send_recorded(
                "YAHOO",
                &recorder,
                &RequestContext::http("test", vec![], serde_json::json!({})),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
    }
}
