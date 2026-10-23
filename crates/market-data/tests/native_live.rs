//! Opt-in checks against public instruments only; no portfolio or credential access.
#![allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
use chrono::{TimeZone, Utc};
use wealthfolio_market_data::{
    EastmoneyProvider, FinancialStatement, InstrumentId, MarketDataProvider, ProviderInstrument,
    QuoteContext, SinaProvider, TencentProvider,
};

#[tokio::test]
#[ignore = "requires live native-provider HTTPS access"]
async fn native_public_market_samples() {
    let providers: Vec<Box<dyn MarketDataProvider>> = vec![
        Box::new(EastmoneyProvider::new().unwrap()),
        Box::new(TencentProvider::new().unwrap()),
        Box::new(SinaProvider::new().unwrap()),
    ];
    let mut failures = Vec::new();
    for p in providers {
        let symbols = match p.id() {
            "EASTMONEY" => vec![
                ("1.600519", "CNY"),
                ("116.00700", "HKD"),
                ("105.AAPL", "USD"),
                ("FUND:005827", "CNY"),
                ("1.510300", "CNY"),
                ("100.SPX", "USD"),
                ("0.920002", "CNY"),
                ("0.161725", "CNY"),
                ("1.000001", "CNY"),
                ("155.HSBA", "GBX"),
            ],
            "TENCENT" => vec![
                ("sh600519", "CNY"),
                ("hk00700", "HKD"),
                ("usAAPL.OQ", "USD"),
                ("sh510300", "CNY"),
                ("usINX", "USD"),
                ("bj920002", "CNY"),
                ("sz161725", "CNY"),
                ("sh000001", "CNY"),
            ],
            _ => vec![
                ("sh600519", "CNY"),
                ("hk00700", "HKD"),
                ("gb_aapl", "USD"),
                ("sh510300", "CNY"),
                ("gb_$inx", "USD"),
                ("bj920002", "CNY"),
                ("sz161725", "CNY"),
                ("sh000001", "CNY"),
            ],
        };
        for (symbol, ccy) in symbols {
            let ctx = QuoteContext {
                instrument: InstrumentId::Equity {
                    ticker: symbol.into(),
                    mic: None,
                },
                identifiers: Default::default(),
                overrides: None,
                currency_hint: Some(ccy.into()),
                preferred_provider: None,
                bond_metadata: None,
                custom_provider_code: None,
            };
            let params = ProviderInstrument::EquitySymbol {
                symbol: symbol.into(),
            };
            let start = Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
            let end = Utc.with_ymd_and_hms(2026, 1, 9, 0, 0, 0).unwrap();
            let latest = p.get_latest_quote(&ctx, params.clone()).await;
            let history = p.get_historical_quotes(&ctx, params, start, end).await;
            let latest_ok = latest
                .as_ref()
                .is_ok_and(|q| q.close > rust_decimal::Decimal::ZERO && q.currency == ccy);
            let history_ok = history.as_ref().is_ok_and(|q| {
                !q.is_empty()
                    && q.iter()
                        .all(|q| q.currency == ccy && q.timestamp >= start && q.timestamp <= end)
            });
            eprintln!(
                "{} {symbol}: latest={} history={}",
                p.id(),
                latest_ok,
                history_ok
            );
            if !latest_ok {
                failures.push(format!("{} {symbol} latest: {:?}", p.id(), latest.err()));
            }
            if !history_ok {
                failures.push(format!("{} {symbol} history: {:?}", p.id(), history.err()));
            }
        }
        match p.search("AAPL").await {
            Ok(rows) if !rows.is_empty() => {}
            r => failures.push(format!("{} search: {r:?}", p.id())),
        }
        if p.id() != "TENCENT" {
            let symbol = if p.id() == "EASTMONEY" {
                "1.600519"
            } else {
                "sh600519"
            };
            match p
                .get_financial_reports(symbol, FinancialStatement::BalanceSheet)
                .await
            {
                Ok(rows) if !rows.is_empty() => {}
                r => failures.push(format!("{} reports: {r:?}", p.id())),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
#[ignore = "requires live news, financial-report and corporate-action endpoints"]
async fn native_research_and_actions() {
    let east = EastmoneyProvider::new().unwrap();
    let sina = SinaProvider::new().unwrap();
    let mut failures = Vec::new();
    match east.get_news("1.600519").await {
        Ok(news)
            if !news.is_empty()
                && news
                    .iter()
                    .all(|n| n.url.starts_with("http") && !n.title.is_empty()) =>
        {
            eprintln!("EASTMONEY news: passed")
        }
        result => failures.push(format!("EASTMONEY news: {result:?}")),
    }
    for symbol in ["116.00700", "105.AAPL"] {
        match east
            .get_financial_reports(symbol, FinancialStatement::BalanceSheet)
            .await
        {
            Ok(reports) if !reports.is_empty() && reports.iter().all(|r| !r.items.is_empty()) => {
                eprintln!("EASTMONEY {symbol} financial reports: {}", reports.len())
            }
            result => failures.push(format!("EASTMONEY {symbol} reports: {result:?}")),
        }
    }
    let context = QuoteContext {
        instrument: InstrumentId::Equity {
            ticker: "600519".into(),
            mic: Some("XSHG".into()),
        },
        identifiers: Default::default(),
        overrides: None,
        currency_hint: Some("CNY".into()),
        preferred_provider: None,
        bond_metadata: None,
        custom_provider_code: None,
    };
    let start = Utc.with_ymd_and_hms(2002, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    for (p, symbol) in [
        (&east as &dyn MarketDataProvider, "1.600519"),
        (&sina as &dyn MarketDataProvider, "sh600519"),
    ] {
        let instrument = ProviderInstrument::EquitySymbol {
            symbol: symbol.into(),
        };
        match p
            .get_dividends(&context, instrument.clone(), start, end)
            .await
        {
            Ok(rows) if !rows.is_empty() => eprintln!("{} dividends: passed", p.id()),
            result => failures.push(format!("{} dividends: {result:?}", p.id())),
        }
        match p.get_splits(&context, instrument, start, end).await {
            Ok(rows) if !rows.is_empty() => eprintln!("{} splits: passed", p.id()),
            result => failures.push(format!("{} splits: {result:?}", p.id())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[tokio::test]
#[ignore = "requires live income and cash-flow report endpoints"]
async fn native_statement_types() {
    let east = EastmoneyProvider::new().unwrap();
    let sina = SinaProvider::new().unwrap();
    let mut failures = Vec::new();
    for (p, symbol) in [
        (&east as &dyn MarketDataProvider, "1.600519"),
        (&east as &dyn MarketDataProvider, "116.00700"),
        (&east as &dyn MarketDataProvider, "105.AAPL"),
        (&sina as &dyn MarketDataProvider, "sh600519"),
    ] {
        for statement in [
            FinancialStatement::IncomeStatement,
            FinancialStatement::CashFlow,
        ] {
            match p.get_financial_reports(symbol, statement).await {
                Ok(rows) if !rows.is_empty() && rows.iter().all(|r| !r.items.is_empty()) => {
                    eprintln!("{} {symbol} {statement:?}: {} reports", p.id(), rows.len())
                }
                result => failures.push(format!("{} {symbol} {statement:?}: {result:?}", p.id())),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
