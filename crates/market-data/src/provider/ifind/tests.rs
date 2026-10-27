use super::*;
use crate::{request_recorder::tests::MemoryRecorder, InstrumentId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

fn samples() -> Value {
    serde_json::from_str(include_str!("fixtures/samples.json")).unwrap()
}
#[test]
fn sampled_market_contracts() {
    let samples = samples();
    for (name, kind, code) in [
        ("stock-get_stock_performance", "stock", "600519.SH"),
        ("fund-get_fund_market_performance", "nav", "005827.OF"),
        ("fund-get_fund_market_performance-2", "fund", "510300.SH"),
        ("index-index_data", "index", "000300.SH"),
        ("global-stock-global_stock_quotes", "global", "AAPL.O"),
        ("global-stock-global_stock_quotes", "global", "00700.HK"),
    ] {
        let data = parse::data(samples[name].clone()).unwrap();
        let rows = parse::history(
            &data,
            kind,
            code,
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 16).unwrap(),
        )
        .unwrap();
        assert_eq!(rows.len(), 5, "{name} {code}");
        assert!(rows
            .iter()
            .all(|q| q.close > Decimal::ZERO && q.source == "IFIND"));
        assert!(rows.windows(2).all(|q| q[0].timestamp < q[1].timestamp));
        if kind == "nav" {
            assert_eq!(rows[0].close, Decimal::from_str_exact("1.4271").unwrap());
            assert!(rows[0].open.is_none());
        }
    }
    for (name, symbol, currency) in [
        ("stock-stock_highfreq_quotes", "600519.SH", "CNY"),
        ("stock-stock_highfreq_quotes", "000001.SZ", "CNY"),
        ("stock-stock_highfreq_quotes", "920002.BJ", "CNY"),
        ("fund-fund_highfreq_quotes", "510300.SH", "CNY"),
        ("fund-fund_highfreq_quotes", "161725.SZ", "CNY"),
        ("index-index_highfreq_quotes", "000300.SH", "CNY"),
    ] {
        let q = parse::latest(
            &parse::data(samples[name].clone()).unwrap(),
            symbol,
            currency,
        )
        .unwrap();
        assert!(q.close > Decimal::ZERO);
        assert_eq!(q.currency, currency);
        assert!(q.volume.is_none());
        assert_eq!(q.timestamp.offset(), &Utc);
    }
}
#[test]
fn search_and_profiles_keep_identity() {
    let samples = samples();
    for (name, kind) in [
        ("stock-search_stocks", "stock"),
        ("fund-search_funds", "fund"),
        ("index-index_data-2", "index"),
        ("global-stock-search_global_stocks", "global"),
    ] {
        let rows = parse::search(&parse::data(samples[name].clone()).unwrap(), kind).unwrap();
        assert!(!rows.is_empty());
        assert!(rows
            .iter()
            .all(|r| r.data_source.as_deref() == Some("IFIND") && r.canonical_symbol.is_some()));
    }
    for (name, kind, code, ccy) in [
        ("stock-get_stock_info", "stock", "600519.SH", "CNY"),
        ("fund-get_fund_profile", "nav", "005827.OF", "CNY"),
        ("fund-get_fund_profile", "fund", "161725.SZ", "CNY"),
        (
            "global-stock-global_stock_profile",
            "global",
            "00700.HK",
            "HKD",
        ),
    ] {
        let p = parse::profile(&parse::data(samples[name].clone()).unwrap(), kind, code).unwrap();
        assert_eq!(p.currency.as_deref(), Some(ccy));
        assert!(p.name.is_some());
    }
    assert_eq!(
        resolve("600519", Some("XSHG")).as_deref(),
        Some("stock:600519.SH")
    );
    assert_eq!(
        resolve("FUND:005827", None).as_deref(),
        Some("nav:005827.OF")
    );
    assert_eq!(
        resolve("00700", Some("XHKG")).as_deref(),
        Some("global:00700.HK")
    );
    assert!(resolve("000001", Some("XSHG")).is_none());
    assert!(resolve("AAPL", None).is_none());
    assert!(identity("index:000001.SH").is_ok());
    let provider = IfindProvider::new("test-key".into()).unwrap();
    assert!(provider.supports_instrument(&InstrumentId::Equity {
        ticker: "FUND:005827".into(),
        mic: None
    }));
    assert!(identity("stock:600519.SH\nquery").is_err());
}
#[test]
fn rejects_wrong_identity_adjustments_currency_and_truncation() {
    let samples = samples();
    let mut data = parse::data(samples["stock-get_stock_performance"].clone()).unwrap();
    let start = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
    assert!(parse::history(&data, "stock", "000001.SZ", start, end).is_err());
    let mut conflicting_volume = data.clone();
    let answer = data["answer"].as_str().unwrap();
    let original = answer.lines().last().unwrap();
    let repeated = original.replace("315.5604万", "1");
    assert_ne!(repeated, original);
    conflicting_volume["answer"] = json!(format!("{answer}\n{repeated}"));
    assert!(parse::history(&conflicting_volume, "stock", "600519.SH", start, end).is_err());
    data["indicators_params"]["收盘价"]["复权方式"] = json!("前复权");
    assert!(parse::history(&data, "stock", "600519.SH", start, end).is_err());
    assert!(parse::data(json!({"code":0,"data":{}})).is_err());
    assert!(parse::data(json!({"code":1,"data":{"answer":"结果已截断"}})).is_err());
    let mut data = parse::data(samples["fund-get_fund_market_performance"].clone()).unwrap();
    data["answer"] = json!(data["answer"]
        .as_str()
        .unwrap()
        .replace("单位净值（单位：元）", "估算净值（单位：元）"));
    assert!(parse::history(&data, "nav", "005827.OF", start, end).is_err());
}

async fn server() -> (
    String,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<(String, Instant)>>>,
) {
    server_with_sse(false).await
}

async fn server_with_sse(
    reconnect: bool,
) -> (
    String,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<(String, Instant)>>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let handle = tokio::spawn(async move {
        let mut streams = 0;
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let (headers, body) = loop {
                let count = stream.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break (String::new(), vec![]);
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break (headers, bytes[end + 4..end + 4 + length].to_vec());
                    }
                }
            };
            if headers.is_empty() {
                continue;
            }
            assert!(headers.to_lowercase().contains("authorization: test-key"));
            assert!(!headers.contains("Bearer"));
            if headers.starts_with("GET") {
                streams += 1;
                if reconnect && streams == 1 {
                    seen.lock().await.push(("GET".into(), Instant::now()));
                    // Control events are not HTTP requests. The closed stream
                    // asks the SDK to reconnect once with the last event ID.
                    let body = "id: checkpoint\nretry: 1\n\n: keepalive\n\n: keepalive\n\n";
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    stream.write_all(response.as_bytes()).await.unwrap();
                    continue;
                }
                if reconnect {
                    assert!(headers.to_lowercase().contains("last-event-id: checkpoint"));
                }
            }
            let (status, body, session) = if !headers.starts_with("POST") {
                seen.lock().await.push((
                    headers.split_whitespace().next().unwrap().to_string(),
                    Instant::now(),
                ));
                ("405 Method Not Allowed", String::new(), false)
            } else {
                let request: Value = serde_json::from_slice(&body).unwrap();
                let method = request["method"].as_str().unwrap();
                seen.lock().await.push((method.to_string(), Instant::now()));
                match method {
                    "initialize" => ("200 OK",json!({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"offline","version":"1"}}}).to_string(),true),
                    "notifications/initialized" => ("202 Accepted",String::new(),false),
                    "tools/call" => {
                        assert!(headers.to_lowercase().contains("mcp-session-id: offline-session"));
                        let name=request["params"]["name"].as_str().unwrap();
                        if name == "slow" {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                        if name == "rpc-error" {
                            let body = json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":-32602, "message":"invalid request"}});
                            ("200 OK", body.to_string(), false)
                        } else if name == "expired" { ("404 Not Found", String::new(), false) } else {
                            let sample = match name {
                                "get_stock_info" => samples()["stock-get_stock_info"].clone(),
                                "get_stock_performance" => {
                                    let query = request["params"]["arguments"]["query"].as_str().unwrap();
                                    let day = &query.split_once('在').unwrap().1[..8];
                                    let mut sample = samples()["stock-get_stock_performance"].clone();
                                    sample["data"]["answer"] = json!(format!("|证券代码|交易币种|日期|收盘价|开盘价|最高价|最低价|成交量|\n|---|---|---|---|---|---|---|---|\n|600519.SH|CNY|{day}|10|10|10|10|100|"));
                                    sample
                                },
                                "get_fund_profile" => samples()["fund-get_fund_profile"].clone(),
                                "global_stock_profile" => samples()["global-stock-global_stock_profile"].clone(),
                                _ => json!({"code":1,"data":{}}),
                            };
                            ("200 OK",json!({"jsonrpc":"2.0","id":request["id"],"result":{"isError":name=="tool-error","content":[{"type":"text","text":sample.to_string()}]}}).to_string(),false)
                        }
                    },
                    _ => panic!("Unexpected MCP method"),
                }
            };
            let session = if session {
                "Mcp-Session-Id: offline-session\r\n"
            } else {
                ""
            };
            let response=format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{session}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (url, handle, calls)
}

async fn wait_for_method(calls: &Mutex<Vec<(String, Instant)>>, method: &str, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if calls
                .lock()
                .await
                .iter()
                .filter(|(m, _)| m == method)
                .count()
                >= count
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn request_counts_sdk_calls_exclude_handshake_and_session_cleanup() {
    let (url, server, calls) = server().await;
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new("test-key".into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = url;
    assert!(provider.get_profile("invalid symbol").await.is_err());
    assert!(provider.search("FUND:005").await.unwrap().is_empty());
    assert!(recorder.0.lock().unwrap().is_empty());

    let before = Utc::now();
    provider.call("stock", "ok", json!({})).await.unwrap();
    wait_for_method(&calls, "GET", 1).await;
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
    provider.call("stock", "ok", json!({})).await.unwrap();
    assert_eq!(recorder.0.lock().unwrap().len(), 2);
    // A cached connection has no handshake to count; invalid tool arguments
    // fail before dispatch and must not replay or invalidate the connection.
    assert!(provider.call("stock", "ok", json!(null)).await.is_err());
    assert_eq!(recorder.0.lock().unwrap().len(), 2);
    let mut connection = provider
        .state
        .lock()
        .await
        .connections
        .remove("stock")
        .unwrap();
    connection.close().await.unwrap();
    let after = Utc::now();
    let calls = calls.lock().await;
    let events = recorder.0.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert!(events
        .iter()
        .all(|(id, time)| id == DATA_SOURCE_IFIND && *time >= before && *time <= after));
    assert_eq!(calls.len(), 6);
    assert_eq!(calls.iter().filter(|(m, _)| m == "initialize").count(), 1);
    assert_eq!(
        calls
            .iter()
            .filter(|(m, _)| m == "notifications/initialized")
            .count(),
        1
    );
    assert_eq!(calls.iter().filter(|(m, _)| m == "DELETE").count(), 1);
    server.abort();
}

#[tokio::test]
async fn initialization_failure_counts_failed_sdk_calls() {
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new("test-key".into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = "not a URL".into();
    assert!(provider.call("stock", "ok", json!({})).await.is_err());
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
    provider.base = "http://127.0.0.1:0".into();
    assert!(provider.call("stock", "ok", json!({})).await.is_err());
    assert_eq!(recorder.0.lock().unwrap().len(), 2);
    assert!(provider.state.lock().await.connections.is_empty());
}

#[tokio::test]
async fn request_counts_sdk_call_does_not_count_sse_reconnects() {
    let (url, server, calls) = server_with_sse(true).await;
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new("test-key".into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = url;
    provider.call("stock", "ok", json!({})).await.unwrap();
    wait_for_method(&calls, "GET", 2).await;
    let mut connection = provider
        .state
        .lock()
        .await
        .connections
        .remove("stock")
        .unwrap();
    connection.close().await.unwrap();
    let calls = calls.lock().await;
    assert_eq!(calls.iter().filter(|(m, _)| m == "GET").count(), 2);
    assert_eq!(calls.iter().filter(|(m, _)| m == "tools/call").count(), 1);
    assert_eq!(calls.iter().filter(|(m, _)| m == "DELETE").count(), 1);
    assert_eq!(calls.len(), 6);
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn request_counts_each_history_window_without_repeating_handshake() {
    let (url, server, calls) = server().await;
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new("test-key".into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = url;
    let start = NaiveDate::from_ymd_opt(2026, 9, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let end = start + chrono::Duration::days(30);
    let rows = provider
        .history("stock:600519.SH", start, end)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].timestamp, start);
    assert_eq!(rows[1].timestamp, end);
    wait_for_method(&calls, "GET", 1).await;
    assert_eq!(recorder.0.lock().unwrap().len(), 2);
    let mut connection = provider
        .state
        .lock()
        .await
        .connections
        .remove("stock")
        .unwrap();
    connection.close().await.unwrap();
    let calls = calls.lock().await;
    assert_eq!(calls.iter().filter(|(m, _)| m == "initialize").count(), 1);
    assert_eq!(calls.iter().filter(|(m, _)| m == "tools/call").count(), 2);
    assert_eq!(calls.len(), 6);
    assert_eq!(recorder.0.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn sdk_handshake_session_serialization_and_expiry_without_replay() {
    let (url, server, calls) = server().await;
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new(" test-key ".into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = url;
    provider.call("stock", "ok", json!({})).await.unwrap();
    let (a, b) = tokio::join!(
        provider.call("stock", "ok", json!({})),
        provider.call("fund", "ok", json!({}))
    );
    assert!(a.is_ok() && b.is_ok());
    assert!(provider
        .call("stock", "tool-error", json!({}))
        .await
        .is_err());
    assert!(provider.call("stock", "expired", json!({})).await.is_err());
    wait_for_method(&calls, "DELETE", 1).await;
    let calls = calls.lock().await;
    assert_eq!(calls.iter().filter(|(m, _)| m == "initialize").count(), 2);
    let tools: Vec<_> = calls.iter().filter(|(m, _)| m == "tools/call").collect();
    assert_eq!(tools.len(), 5);
    assert_eq!(calls.len(), 12);
    assert_eq!(recorder.0.lock().unwrap().len(), 5);
    assert!(tools
        .windows(2)
        .all(|pair| pair[1].1.duration_since(pair[0].1) >= Duration::from_millis(480)));
    server.abort();
}

#[test]
fn published_prices_require_real_dates_and_history_does_not_invent_trading_days() {
    let samples = samples();
    let data = parse::data(samples["global-stock-global_stock_quotes-2"].clone()).unwrap();
    let q = parse::latest_daily(&data, "global", "00700.HK").unwrap();
    assert_eq!(q.currency, "HKD");
    assert_eq!(
        q.timestamp.date_naive(),
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()
    );
    let mut data = data;
    data["answer"] = json!(data["answer"]
        .as_str()
        .unwrap()
        .replace("最近交易日期", "未知日期"));
    assert!(parse::latest_daily(&data, "global", "00700.HK").is_err());
    let mut data = parse::data(samples["stock-get_stock_performance"].clone()).unwrap();
    let answer = data["answer"].as_str().unwrap().to_string();
    let header = answer.lines().find(|line| line.contains("日期")).unwrap();
    let fields: Vec<_> = header.trim_matches('|').split('|').collect();
    let fake: Vec<_> = fields
        .iter()
        .map(|field| match *field {
            "证券代码" => "600519.SH",
            "证券简称" => "贵州茅台",
            "日期" => "20261017",
            "收盘价" => "1329.0",
            _ => "",
        })
        .collect();
    data["answer"] = json!(format!("{answer}\n|{}|", fake.join("|")));
    let rows = parse::history(
        &data,
        "stock",
        "600519.SH",
        NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
    )
    .unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].volume, Some(Decimal::from(3155604)));
    let mut data = parse::data(samples["stock-get_stock_performance"].clone()).unwrap();
    let answer = data["answer"].as_str().unwrap();
    let repeated = answer.lines().last().unwrap().replace("1288.0", "1289.0");
    data["answer"] = json!(format!("{answer}\n{repeated}"));
    assert!(parse::history(
        &data,
        "stock",
        "600519.SH",
        NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
        NaiveDate::from_ymd_opt(2026, 10, 16).unwrap()
    )
    .is_err());
}

#[tokio::test]
async fn rpc_errors_and_timeouts_propagate_without_replay() {
    for tool in ["rpc-error", "slow"] {
        let (url, server, calls) = server().await;
        let mut provider = IfindProvider::new("test-key".into()).unwrap();
        provider.base = url;
        let result = provider.call("stock", tool, json!({})).await;
        if tool == "slow" {
            assert!(matches!(result, Err(MarketDataError::Timeout { .. })));
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            calls
                .lock()
                .await
                .iter()
                .filter(|(method, _)| method == "tools/call")
                .count(),
            1
        );
        server.abort();
    }
}

#[tokio::test]
async fn qualified_search_preserves_identity_in_provider_and_registry() {
    use crate::{registry::ProviderRegistry, resolver::ResolverChain};
    let (url, server, calls) = server().await;
    let mut provider = IfindProvider::new("test-key".into()).unwrap();
    provider.base = url;
    let provider = Arc::new(provider);
    let registry = ProviderRegistry::new(vec![provider.clone()], Arc::new(ResolverChain::new()));
    for (query, symbol, canonical, mic) in [
        ("FUND:005827", "nav:005827.OF", "FUND:005827", None),
        ("XSHG:600519", "stock:600519.SH", "600519", Some("XSHG")),
        ("XSHG:510300", "fund:510300.SH", "510300", Some("XSHG")),
        ("XSHE:161725", "fund:161725.SZ", "161725", Some("XSHE")),
        ("XHKG:00700", "global:00700.HK", "00700", Some("XHKG")),
        ("XNAS:AAPL", "global:AAPL.O", "AAPL", Some("XNAS")),
        (" fund:005827 ", "nav:005827.OF", "FUND:005827", None),
    ] {
        for rows in [
            provider.search(query).await.unwrap(),
            registry
                .search(query)
                .await
                .unwrap_or_else(|error| panic!("{query}: {error}")),
        ] {
            assert_eq!(rows.len(), 1, "{query}");
            assert_eq!(rows[0].symbol, symbol);
            assert_eq!(rows[0].canonical_symbol.as_deref(), Some(canonical));
            assert_eq!(rows[0].exchange_mic.as_deref(), mic);
        }
    }
    let tool_count = || async {
        calls
            .lock()
            .await
            .iter()
            .filter(|(method, _)| method == "tools/call")
            .count()
    };
    // Each explicit identity needs one profile, never a fuzzy cross-market search.
    assert_eq!(tool_count().await, 14);
    for query in ["FUND:005", "XSHG:000001", "XTSE:SHOP"] {
        assert!(provider.search(query).await.unwrap().is_empty());
        assert!(registry.search(query).await.is_err());
    }
    assert_eq!(tool_count().await, 14);
    // The profile response for this market contains only 600519.SH.
    assert!(provider.search("XSHG:600000").await.is_err());
    server.abort();
}
#[tokio::test]
async fn sdk_diagnostics_capture_safe_arguments_symbols_and_results() {
    use crate::request_recorder::tests::assert_safe_context;
    let (url, server, _) = server().await;
    let secret = "test-key";
    let recorder = Arc::new(MemoryRecorder::default());
    let mut provider = IfindProvider::new(secret.into())
        .unwrap()
        .with_request_recorder(recorder.clone());
    provider.base = url;
    provider
        .call_with_symbols(
            "stock",
            "ok",
            json!({"query":"600519.SH的最新价", "token":secret, "unexpected_private_field":secret}),
            vec!["600519.SH".into()],
        )
        .await
        .unwrap();
    let context = assert_safe_context(&recorder, secret);
    assert_eq!(context.record_kind, "sdk");
    assert_eq!(context.symbols, ["600519.SH"]);
    assert_eq!(context.parameters["query"], "600519.SH的最新价");
    assert_eq!(recorder.2.lock().unwrap()[0].1.outcome, "success");
    assert!(provider
        .call("stock", "tool-error", json!({}))
        .await
        .is_err());
    assert_eq!(recorder.2.lock().unwrap()[1].1.outcome, "failure");
    server.abort();
}
