//! Observable application sends for the chart/search subset of yahoo_finance_api
//! 5.0.0. Keep its URLs, public parsers, response classification and single chart
//! retry: counting SDK method calls would hide that retry. No reqwest policy is
//! changed. Recheck these compatibility rules when upgrading the SDK.

use super::*;
use reqwest::StatusCode;
use serde_json::Value;

const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart";
const SEARCH_URL: &str = "https://query2.finance.yahoo.com/v1/finance/search";

impl YahooProvider {
    pub(super) async fn chart_latest(
        &self,
        symbol: &str,
    ) -> Result<yahoo::YResponse, yahoo::YahooError> {
        self.chart_request(&latest_url(CHART_URL, symbol)).await
    }

    pub(super) async fn chart_history(
        &self,
        symbol: &str,
        start: OffsetDateTime,
        end: OffsetDateTime,
        interval: &str,
    ) -> Result<yahoo::YResponse, yahoo::YahooError> {
        self.chart_request(&history_url(CHART_URL, symbol, start, end, interval))
            .await
    }

    pub(super) async fn search_ticker_recorded(
        &self,
        name: &str,
    ) -> Result<yahoo::YSearchResult, yahoo::YahooError> {
        // Callers already encode the query. The SDK encoded it again; preserve
        // that existing behavior rather than mixing a search fix into counting.
        let json = self.sdk_json(&search_url(SEARCH_URL, name)).await?;
        let result = yahoo::YSearchResultOpt::from_json(json)?;
        Ok(yahoo::YSearchResult::from_opt(&result))
    }

    async fn sdk_json(&self, url: &str) -> Result<Value, yahoo::YahooError> {
        let response = self
            .client
            .get(url)
            .send_recorded("YAHOO", self.request_recorder.as_ref())
            .await?;
        let status = response.status();
        decode_body(&response.text().await?, status, url)
    }

    async fn chart_request(&self, url: &str) -> Result<yahoo::YResponse, yahoo::YahooError> {
        let mut result = self.sdk_json(url).await;
        if matches!(
            &result,
            Err(yahoo::YahooError::EmptyResponse
                | yahoo::YahooError::HtmlResponse
                | yahoo::YahooError::ServerError(_))
        ) {
            result = self.sdk_json(url).await;
        }
        let mut response = yahoo::YResponse::from_json(result?)?;
        // Equivalent to the SDK's private map_error_msg, after public parsing.
        if response.chart.result.as_ref().is_none_or(Vec::is_empty) {
            if let Some(error) = response.chart.error.take() {
                return Err(yahoo::YahooError::ApiError(error));
            }
        }
        Ok(response)
    }
}

fn latest_url(base: &str, symbol: &str) -> String {
    let symbol = encode(symbol);
    format!("{base}/{symbol}?symbol={symbol}&interval=1d&range=1mo&events=div|split|capitalGains")
}

fn history_url(
    base: &str,
    symbol: &str,
    start: OffsetDateTime,
    end: OffsetDateTime,
    interval: &str,
) -> String {
    let symbol = encode(symbol);
    format!(
        "{base}/{symbol}?symbol={symbol}&period1={}&period2={}&interval={interval}&events=div|split|capitalGains",
        start.unix_timestamp(), end.unix_timestamp()
    )
}

fn search_url(base: &str, name: &str) -> String {
    format!("{base}?q={}", encode(name))
}

fn api_error(json: &Value, url: &str) -> Option<yahoo::YahooError> {
    [
        json.get("error"),
        json.get("chart").and_then(|c| c.get("error")),
    ]
    .into_iter()
    .flatten()
    .find_map(|error| {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())?;
        let description = error.get("description").and_then(Value::as_str);
        Some(match code {
            "Too Many Requests" => yahoo::YahooError::TooManyRequests(url.into()),
            "Unauthorized" => yahoo::YahooError::Unauthorized,
            "Invalid Crumb" => yahoo::YahooError::InvalidCrumb,
            _ => yahoo::YahooError::ApiError(
                serde_json::from_value(
                    serde_json::json!({"code": code, "description": description}),
                )
                .expect("Yahoo API error consists of optional string fields"),
            ),
        })
    })
}

/// Compatibility with the SDK's private decoder, including precedence of HTTP
/// status over body and no retry for rate limits, invalid JSON or API errors.
fn decode_body(text: &str, status: StatusCode, url: &str) -> Result<Value, yahoo::YahooError> {
    use yahoo::YahooError;
    match status {
        StatusCode::TOO_MANY_REQUESTS => return Err(YahooError::TooManyRequests(url.into())),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(YahooError::Unauthorized),
        StatusCode::NOT_FOUND => {
            return Err(YahooError::FetchFailed(format!("404, request url: {url}")))
        }
        _ => {}
    }
    if status.is_server_error() {
        return Err(YahooError::ServerError(format!(
            "{} status, request url: {url}",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        if let Ok(json) = serde_json::from_str::<Value>(text.trim()) {
            if let Some(error) = api_error(&json, url) {
                return Err(error);
            }
        }
        return Err(YahooError::FetchFailed(format!(
            "{} status, request url: {url}",
            status.as_u16()
        )));
    }
    let body = text.trim().trim_start_matches('\u{feff}');
    if body.is_empty() {
        return Err(YahooError::EmptyResponse);
    }
    let is_json = body.starts_with('{') || body.starts_with('[');
    if body.starts_with('<') || (!is_json && body.contains("Will be right back")) {
        return Err(YahooError::HtmlResponse);
    }
    if !is_json && body.to_ascii_lowercase().contains("too many requests") {
        return Err(YahooError::TooManyRequests(url.into()));
    }
    let json: Value = serde_json::from_str(body)?;
    if json.get("status_code").is_some() {
        return Err(YahooError::FetchFailed(format!(
            "Yahoo returned status_code in body, request url: {url}"
        )));
    }
    if let Some(error) = api_error(&json, url) {
        return Err(error);
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_recorder::tests::{MemoryRecorder, MockServer};
    use std::sync::Arc;

    const CHART: &str = r#"{"chart":{"result":[],"error":null}}"#;

    fn provider(recorder: Arc<MemoryRecorder>) -> YahooProvider {
        YahooProvider {
            client: wealthfolio_http::client(),
            request_recorder: recorder,
        }
    }

    #[test]
    fn sdk_request_parameters_and_double_encoding_are_preserved() {
        assert_eq!(latest_url("https://example.test/chart", "^A &+"),
            "https://example.test/chart/%5EA%20%26%2B?symbol=%5EA%20%26%2B&interval=1d&range=1mo&events=div|split|capitalGains");
        let start = OffsetDateTime::from_unix_timestamp(100).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(200).unwrap();
        for interval in ["1d", "3mo"] {
            assert_eq!(history_url("https://example.test/chart", "BRK-B", start, end, interval),
                format!("https://example.test/chart/BRK-B?symbol=BRK-B&period1=100&period2=200&interval={interval}&events=div|split|capitalGains"));
        }
        assert_eq!(
            search_url("https://example.test/search", &encode("A &B")),
            "https://example.test/search?q=A%2520%2526B"
        );
    }

    #[tokio::test]
    async fn chart_counts_each_sdk_equivalent_retry_with_unchanged_uri() {
        for (status, body) in [
            (500, "failure"),
            (200, ""),
            (200, "<html>maintenance</html>"),
        ] {
            let server = MockServer::start(vec![
                (status, String::new(), body.into()),
                (200, String::new(), CHART.into()),
            ]);
            let recorder = Arc::new(MemoryRecorder::default());
            let provider = provider(recorder.clone());
            let url = latest_url(&server.url, "TEST");
            provider.chart_request(&url).await.unwrap();
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], requests[1]);
            assert!(requests[0].starts_with("GET /TEST?symbol=TEST&interval=1d&range=1mo&events=div|split|capitalGains HTTP/1.1"));
            let events = recorder.0.lock().unwrap();
            assert_eq!(events.len(), 2);
            assert!(events.iter().all(|(id, _)| id == "YAHOO"));
        }
    }

    #[tokio::test]
    async fn chart_retry_exhaustion_stops_after_two_requests() {
        let server = MockServer::start(vec![
            (503, String::new(), "outage".into()),
            (503, String::new(), "outage".into()),
        ]);
        let recorder = Arc::new(MemoryRecorder::default());
        let result = provider(recorder.clone()).chart_request(&server.url).await;
        assert!(matches!(result, Err(yahoo::YahooError::ServerError(_))));
        assert_eq!(recorder.0.lock().unwrap().len(), 2);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn chart_does_not_retry_rate_limit_auth_or_invalid_json() {
        for (status, body) in [
            (429, "limited"),
            (401, "auth"),
            (403, "auth"),
            (200, "not json"),
            (200, r#"{"chart":{"error":{"code":"Invalid Crumb"}}}"#),
        ] {
            let server = MockServer::start(vec![(status, String::new(), body.into())]);
            let recorder = Arc::new(MemoryRecorder::default());
            assert!(provider(recorder.clone())
                .chart_request(&server.url)
                .await
                .is_err());
            assert_eq!(recorder.0.lock().unwrap().len(), 1);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn chart_does_not_retry_connection_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let recorder = Arc::new(MemoryRecorder::default());
        assert!(provider(recorder.clone())
            .chart_request(&url)
            .await
            .is_err());
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn response_classification_matches_sdk_5_0_0_contract() {
        use yahoo::YahooError;
        let decode = |body, status| decode_body(body, StatusCode::from_u16(status).unwrap(), "url");
        assert!(matches!(
            decode("<html>", 429),
            Err(YahooError::TooManyRequests(_))
        ));
        assert!(matches!(decode("", 403), Err(YahooError::Unauthorized)));
        assert!(matches!(decode("", 404), Err(YahooError::FetchFailed(_))));
        assert!(matches!(decode("", 502), Err(YahooError::ServerError(_))));
        assert!(matches!(
            decode("\u{feff}", 200),
            Err(YahooError::EmptyResponse)
        ));
        assert!(matches!(
            decode("Will be right back", 200),
            Err(YahooError::HtmlResponse)
        ));
        assert!(matches!(
            decode("Too Many Requests", 200),
            Err(YahooError::TooManyRequests(_))
        ));
        assert!(matches!(
            decode(r#"{"status_code":200}"#, 200),
            Err(YahooError::FetchFailed(_))
        ));
        assert!(matches!(
            decode(r#"{"error":"Unauthorized"}"#, 400),
            Err(YahooError::Unauthorized)
        ));
        assert!(matches!(
            decode(r#"{"chart":{"error":{"code":"Invalid Crumb"}}}"#, 200),
            Err(YahooError::InvalidCrumb)
        ));
        assert!(matches!(
            decode(
                r#"{"chart":{"error":{"code":"InvalidPeriod","description":"too many requests"}}}"#,
                200
            ),
            Err(YahooError::ApiError(_))
        ));
        assert!(matches!(
            decode("{", 200),
            Err(YahooError::DeserializeFailed(_))
        ));
        assert_eq!(
            decode(&format!("\u{feff}{CHART}"), 200).unwrap()["chart"]["result"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    async fn chart_error_without_code_still_uses_sdk_error_mapping() {
        let server = MockServer::start(vec![(
            200,
            String::new(),
            r#"{"chart":{"result":null,"error":{"description":"no data"}}}"#.into(),
        )]);
        let recorder = Arc::new(MemoryRecorder::default());
        let result = provider(recorder.clone()).chart_request(&server.url).await;
        assert!(matches!(result, Err(yahoo::YahooError::ApiError(_))));
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
    }
}
