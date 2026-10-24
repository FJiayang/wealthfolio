//! Profile-owned observation of downstream API request dispatches.
//!
//! Record only the provider identity and UTC dispatch timestamp. Implementations
//! must fail open: an unavailable statistics store must not change the request
//! result or cause the caller to retry a network operation.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait RequestRecorder: Send + Sync {
    async fn record_request(&self, provider_id: &str, dispatched_at: DateTime<Utc>);
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
    ) -> Result<reqwest::Response, reqwest::Error>;
}

#[async_trait]
impl RecordedRequest for reqwest::RequestBuilder {
    async fn send_recorded(
        self,
        provider_id: &str,
        recorder: &dyn RequestRecorder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let (client, request) = self.build_split();
        let request = request?;
        recorder.record_request(provider_id, Utc::now()).await;
        client.execute(request).await
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
    pub(crate) struct MemoryRecorder(pub Mutex<Vec<(String, DateTime<Utc>)>>);

    #[async_trait]
    impl RequestRecorder for MemoryRecorder {
        async fn record_request(&self, provider_id: &str, dispatched_at: DateTime<Utc>) {
            self.0
                .lock()
                .unwrap()
                .push((provider_id.into(), dispatched_at));
        }
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
                .send_recorded("EASTMONEY", &recorder)
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
    }

    #[tokio::test]
    async fn local_builder_errors_do_not_count_but_connection_failures_do() {
        let recorder = MemoryRecorder::default();
        let client = wealthfolio_http::client();
        assert!(client
            .get("not a URL")
            .send_recorded("SINA", &recorder)
            .await
            .is_err());
        assert!(recorder.0.lock().unwrap().is_empty());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert!(client
            .get(url)
            .send_recorded("SINA", &recorder)
            .await
            .is_err());
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
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
            .send_recorded("YAHOO", &recorder)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
    }
}
