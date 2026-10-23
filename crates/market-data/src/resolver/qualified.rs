use crate::is_known_mic;

/// Qualified identifiers are explicit; bare codes must never imply a market.
pub fn qualified_symbol(symbol: &str) -> Option<(&str, &str)> {
    let (market, code) = symbol.split_once(':')?;
    if (market != "FUND" && !is_known_mic(market))
        || market.is_empty()
        || code.is_empty()
        || !market
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || !code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return None;
    }
    Some((market, code))
}
