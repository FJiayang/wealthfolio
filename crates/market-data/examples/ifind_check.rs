//! Explicit live acceptance: THS_KEY is read only by this executable, never by tests.
use chrono::{TimeZone, Utc};
use wealthfolio_market_data::{
    IfindProvider, InstrumentId, MarketDataProvider, ProviderInstrument, QuoteContext,
};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("THS_KEY").map_err(|_| "THS_KEY is not configured")?;
    let provider = IfindProvider::new(key)?;
    let mut failures = 0;
    let search_only = std::env::args().any(|arg| arg == "--search-only");
    for (symbol, ccy) in [
        ("stock:600519.SH", "CNY"),
        ("stock:000001.SZ", "CNY"),
        ("stock:920002.BJ", "CNY"),
        ("fund:510300.SH", "CNY"),
        ("fund:161725.SZ", "CNY"),
        ("nav:005827.OF", "CNY"),
        ("index:000300.SH", "CNY"),
        ("global:00700.HK", "HKD"),
        ("global:AAPL.O", "USD"),
    ]
    .into_iter()
    .filter(|_| !search_only)
    {
        let profile = provider.get_profile(symbol).await;
        let profile_ok = profile
            .as_ref()
            .is_ok_and(|p| p.currency.as_deref() == Some(ccy));
        let ctx = QuoteContext {
            instrument: InstrumentId::Equity {
                ticker: symbol.into(),
                mic: None,
            },
            identifiers: Default::default(),
            overrides: None,
            currency_hint: Some(ccy.to_string().into()),
            preferred_provider: None,
            bond_metadata: None,
            custom_provider_code: None,
        };
        let instrument = ProviderInstrument::EquitySymbol {
            symbol: symbol.into(),
        };
        let latest = provider.get_latest_quote(&ctx, instrument.clone()).await;
        let latest_ok = latest
            .as_ref()
            .is_ok_and(|q| q.currency == ccy && q.close > rust_decimal::Decimal::ZERO);
        let history = provider
            .get_historical_quotes(
                &ctx,
                instrument,
                Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0)
                    .single()
                    .ok_or("invalid start")?,
                Utc.with_ymd_and_hms(2026, 10, 16, 0, 0, 0)
                    .single()
                    .ok_or("invalid end")?,
            )
            .await;
        let history_ok = history
            .as_ref()
            .is_ok_and(|q| q.len() == 5 && q.iter().all(|q| q.currency == ccy));
        println!("{symbol}: profile={profile_ok} latest={latest_ok} history={history_ok}");
        for (operation, error) in [
            ("profile", profile.as_ref().err()),
            ("latest", latest.as_ref().err()),
            ("history", history.as_ref().err()),
        ] {
            if let Some(error) = error {
                println!("{symbol} {operation}: {error}");
            }
        }
        // Errors are deliberately summarized; SDK errors can contain upstream response bodies.
        if !(profile_ok && latest_ok && history_ok) {
            failures += 1;
        }
    }
    for query in [
        "贵州茅台",
        "易方达蓝筹精选混合",
        "沪深300",
        "腾讯控股",
        "苹果",
    ] {
        let rows = provider.search(query).await;
        let count = rows.as_ref().map_or(0, Vec::len);
        println!("search {query}: count={count}");
        if count == 0 {
            failures += 1;
        }
    }
    if failures > 0 {
        return Err(format!("{failures} live scenarios failed").into());
    }
    Ok(())
}
