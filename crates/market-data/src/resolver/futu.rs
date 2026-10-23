use super::{qualified_symbol, strip_yahoo_suffix_for_mic, yahoo_suffix_to_mic};

pub(crate) fn resolve(ticker: &str, mic: Option<&str>) -> Option<String> {
    let (ticker, mic) = if let Some((market, code)) = qualified_symbol(ticker) {
        if mic.is_some_and(|mic| mic != market) {
            return None;
        }
        (code, Some(market))
    } else {
        (ticker, mic)
    };
    let suffix_mic = ticker
        .rsplit_once('.')
        .and_then(|(_, suffix)| yahoo_suffix_to_mic(suffix));
    if mic
        .zip(suffix_mic)
        .is_some_and(|(mic, suffix)| mic != suffix)
    {
        return None;
    }
    let mic = mic.or(suffix_mic);
    let code = mic
        .and_then(|mic| strip_yahoo_suffix_for_mic(ticker, mic))
        .unwrap_or(ticker);
    let market = match mic {
        Some("XNAS" | "XNYS" | "XASE" | "ARCX" | "BATS") => "US",
        Some("XHKG") => "HK",
        Some("XSHG") => "SH",
        Some("XSHE") => "SZ",
        Some("BJSE") => "BJ",
        None if code
            .bytes()
            .all(|c| c.is_ascii_alphabetic() || b".-".contains(&c))
            && code.bytes().any(|c| c.is_ascii_alphabetic()) =>
        {
            "US"
        }
        _ => return None,
    };
    let code = if market == "US" {
        if super::has_unrecognized_dotted_suffix(code) {
            return None;
        }
        code.replace('-', ".").to_uppercase()
    } else {
        if !code.bytes().all(|c| c.is_ascii_digit()) || code.is_empty() {
            return None;
        }
        if market == "HK" {
            if code.len() > 5 {
                return None;
            }
            format!("{code:0>5}")
        } else {
            if code.len() != 6 {
                return None;
            }
            code.to_owned()
        }
    };
    let symbol = format!("{market}.{code}");
    market_info(&symbol)?;
    Some(symbol)
}

pub(crate) fn market_info(symbol: &str) -> Option<(&'static str, chrono_tz::Tz)> {
    let (market, code) = symbol.split_once('.')?;
    match market {
        "US" if !code.is_empty()
            && code
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)) =>
        {
            Some(("USD", chrono_tz::America::New_York))
        }
        "HK" if code.len() == 5 && code.bytes().all(|c| c.is_ascii_digit()) => {
            Some(("HKD", chrono_tz::Asia::Hong_Kong))
        }
        "SH" | "SZ" | "BJ" if code.len() == 6 && code.bytes().all(|c| c.is_ascii_digit()) => {
            Some(("CNY", chrono_tz::Asia::Shanghai))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn futu_resolves_only_known_markets() {
        for (ticker, mic, expected) in [
            ("700.HK", None, "HK.00700"),
            ("700", Some("XHKG"), "HK.00700"),
            ("600519.SS", None, "SH.600519"),
            ("000001.SZ", None, "SZ.000001"),
            ("BJSE:920002", None, "BJ.920002"),
            ("XSHG:600519", None, "SH.600519"),
            ("XSHE:161725", None, "SZ.161725"),
            ("XHKG:700", None, "HK.00700"),
            ("XNAS:AAPL", None, "US.AAPL"),
            ("AAPL", None, "US.AAPL"),
            ("BRK-B", Some("XNYS"), "US.BRK.B"),
            ("BRK.B", None, "US.BRK.B"),
        ] {
            assert_eq!(resolve(ticker, mic).as_deref(), Some(expected));
        }
        for (ticker, mic) in [
            ("600519", None),
            ("SHOP", Some("XTSE")),
            ("700.HK", Some("XSHG")),
            ("FUND:005827", None),
            ("ABC.ZZ", None),
            ("^HSI", None),
        ] {
            assert!(resolve(ticker, mic).is_none(), "{ticker}");
        }
    }
}
