//! Fixed daily-bar decoder translated from AKShare's stock/cons.py (MIT).
//! The upstream response is data, never executable code. See THIRD_PARTY_LICENSES.md.
use super::super::native::invalid;
use crate::errors::MarketDataError;
use chrono::{Duration, NaiveDate};
use num_traits::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};

#[derive(Debug)]
pub struct Bar {
    pub date: NaiveDate,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}
struct Bits {
    data: Vec<u8>,
    pos: usize,
}
impl Bits {
    fn new(text: &str) -> Result<Self, MarketDataError> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let data = text
            .bytes()
            .map(|b| {
                ALPHABET
                    .iter()
                    .position(|&c| c == b)
                    .map(|n| n as u8)
                    .ok_or_else(|| invalid("Invalid compressed history alphabet"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { data, pos: 0 })
    }
    fn remaining(&self) -> usize {
        self.data.len() * 6 - self.pos
    }
    fn read(&mut self, n: i32, signed: bool) -> Result<i64, MarketDataError> {
        if !(0..=53).contains(&n) || self.remaining() < n as usize {
            return Err(invalid("Invalid compressed history bit width"));
        }
        let mut value = 0i64;
        for i in 0..n {
            value |= (((self.data[self.pos / 6] >> (self.pos % 6)) & 1) as i64) << i;
            self.pos += 1;
        }
        if signed && n > 0 && value & (1i64 << (n - 1)) != 0 {
            value -= 1i64 << n;
        }
        Ok(value)
    }
    fn bit(&mut self) -> Result<bool, MarketDataError> {
        Ok(self.read(1, false)? != 0)
    }
    fn delta(&mut self) -> Result<i32, MarketDataError> {
        let sign = if self.bit()? { 1 } else { -1 };
        let mut n = 1;
        while self.bit()? {
            n += 1;
            if n > 53 {
                return Err(invalid("Invalid compressed delta"));
            }
        }
        Ok(sign * n)
    }
    fn days(&mut self, day: &mut i64) -> Result<i64, MarketDataError> {
        let n = self.read(3, false)?;
        match n {
            1 => {
                *day = self.read(18, true)?;
                Ok(0)
            }
            0 => self.read(6, false),
            _ => Ok(n),
        }
    }
}
fn advance(day: &mut i64, count: i64, weekdays: i64) -> Result<NaiveDate, MarketDataError> {
    if !(0..=1000).contains(&count) || weekdays == 0 {
        return Err(invalid("Invalid compressed calendar"));
    }
    for _ in 0..count {
        loop {
            *day += 1;
            if weekdays & (1 << ((*day % 7 + 10) % 7)) != 0 {
                break;
            }
        }
    }
    NaiveDate::from_ymd_opt(1990, 12, 19)
        .expect("Sina calendar epoch")
        .checked_add_signed(Duration::days(*day))
        .ok_or_else(|| invalid("Compressed date overflow"))
}
fn powers(p: i32) -> (i32, i32) {
    if p < 0 {
        let (a, b) = powers(-p);
        return (-a, -b);
    }
    (p / 3 + i32::from(p % 3 == 1), p / 3 + i32::from(p % 3 == 2))
}
fn scaled(value: i64, p: i32) -> Result<Decimal, MarketDataError> {
    if !(-24..=36).contains(&p) {
        return Err(invalid("Invalid compressed precision"));
    }
    let (a, b) = powers(p);
    let mut value = Decimal::from(value);
    for (base, n) in [(2, a), (5, b)] {
        for _ in 0..n.abs() {
            value = if n >= 0 {
                value.checked_div(Decimal::from(base))
            } else {
                value.checked_mul(Decimal::from(base))
            }
            .ok_or_else(|| invalid("Compressed price overflow"))?;
        }
    }
    Ok(value)
}
fn rescale(value: i64, old: i32, new: i32) -> Result<i64, MarketDataError> {
    scaled(value, old)?
        .checked_div(scaled(1, new)?)
        .ok_or_else(|| invalid("Invalid precision divisor"))?
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        .to_i64()
        .ok_or_else(|| invalid("Compressed precision overflow"))
}
pub fn decode(text: &str) -> Result<Vec<Bar>, MarketDataError> {
    let mut b = Bits::new(text)?;
    let kind = b.read(12, false)?;
    let version = 63 ^ b.read(6, false)?;
    if version != 0 {
        return Err(invalid("Unsupported daily history version"));
    }
    match kind {
        1479 => legacy(&mut b),
        3466 => modern(&mut b),
        _ => Err(invalid("Unsupported compressed daily history")),
    }
}
fn add(a: i64, b: i64) -> Result<i64, MarketDataError> {
    a.checked_add(b)
        .ok_or_else(|| invalid("Compressed accumulator overflow"))
}
fn legacy(b: &mut Bits) -> Result<Vec<Bar>, MarketDataError> {
    let precision = b.read(6, false)?;
    if precision > 12 {
        return Err(invalid("Invalid legacy precision"));
    }
    let divisor = Decimal::from(10i64.pow(precision as u32));
    let mut day = b.read(18, true)? - 1;
    let md = b.read(3, false)? as i32;
    let mv = b.read(3, false)? as i32;
    let (mut ld, mut lv, mut close, mut volume) = (0, 0, 0i64, 0i64);
    let mut out = Vec::new();
    while b.remaining() >= 6 {
        let c = b.read(6, false)?;
        let mut days = 1;
        let (mut ud, mut uv) = (None, None);
        if c & 32 != 0 {
            loop {
                let a = b.read(6, false)?;
                if 16 | a == 63 {
                    let d = b.read(3, false)? as i32 + md;
                    let v = b.read(3, false)? as i32 + mv;
                    if a & 16 != 0 {
                        ld = d;
                        lv = v;
                    }
                    ud = Some(d);
                    uv = Some(v);
                    break;
                }
                if a & 32 != 0 {
                    let value = (a & 7) as i32 + if a & 8 != 0 { md } else { mv };
                    if a & 8 != 0 {
                        ud = Some(value);
                        if a & 16 != 0 {
                            ld = value;
                        }
                    } else {
                        uv = Some(value);
                        if a & 16 != 0 {
                            lv = value;
                        }
                    }
                    break;
                }
                let d = a & 15;
                days = match d {
                    0 => b.read(6, false)?,
                    1 => {
                        day = b.read(18, false)?;
                        0
                    }
                    _ => d,
                };
                if a & 16 == 0 {
                    break;
                }
            }
        }
        let date = advance(&mut day, days, 62)?;
        let ud = ud.unwrap_or(ld);
        let uv = uv.unwrap_or(lv);
        let mut widths = [ud, ud, ud, ud, uv];
        let mut mask =
            [0, 3, 5, 6, 9, 10, 12, 15, 17, 18, 20, 23, 24, 27, 29, 30][(c & 15) as usize];
        if uv & 1 != 0 {
            mask = 31 - mask;
        }
        if c & 16 != 0 {
            widths[4] += 2;
        }
        let mut values = [0i64; 5];
        for i in 0..5 {
            widths[i] += i32::from(mask & (1 << (4 - i)) != 0);
            values[i] = b.read(widths[i] * 3, matches!(i, 0 | 3 | 4))?;
        }
        let open = add(close, values[0])?;
        close = add(open, values[3])?;
        volume = add(volume, values[4])?;
        out.push(Bar {
            date,
            open: Decimal::from(open) / divisor,
            high: Decimal::from(add(open, values[1])?) / divisor,
            low: Decimal::from(add(open, -values[2])?) / divisor,
            close: Decimal::from(close) / divisor,
            volume: Decimal::from(volume),
        });
    }
    Ok(out)
}
fn modern(b: &mut Bits) -> Result<Vec<Bar>, MarketDataError> {
    let (mut average, mut post, mut extra, mut separate) = (true, false, false, false);
    let mut precision = [6, 0, 0, 0, 0]; // price, volume, amount, post volume, post amount
    let mut widths = [3, 3, 3, 3, 5, 5, 3, 0]; // ohlcvaet
    let mut previous = [0i64; 8];
    let mut last_price = 0i64;
    let mut weekdays = 62;
    let mut day = 0;
    let mut out = Vec::new();
    loop {
        let mut days = 1;
        let mut controls = 0;
        if b.bit()? {
            if b.bit()? {
                if b.bit()? {
                    controls += 1;
                    if b.bit()? {
                        average ^= b.bit()?;
                        post ^= b.bit()?;
                        extra ^= b.bit()?;
                        let old = separate;
                        separate ^= b.bit()?;
                        if b.bit()? {
                            weekdays = b.read(7, false)?;
                        }
                        if old != separate {
                            if old {
                                last_price = previous[3];
                            } else {
                                previous[..4].fill(last_price);
                            }
                        }
                    }
                    for i in 0..(3 + 2 * usize::from(post)) {
                        if b.bit()? {
                            let old = precision[i];
                            precision[i] += b.delta()?;
                            if i == 0 {
                                last_price = rescale(last_price, old, precision[0])?;
                                if separate {
                                    for p in &mut previous[..4] {
                                        *p = rescale(*p, old, precision[0])?;
                                    }
                                }
                            } else if i == 1 {
                                previous[4] = rescale(previous[4], old, precision[1])?;
                            }
                        }
                    }
                }
                if b.bit()? {
                    controls += 1;
                    for i in 0..(7 + usize::from(post) + usize::from(extra)) {
                        if b.bit()? {
                            if i == 6 {
                                days = b.days(&mut day)?;
                            } else {
                                let index = if i > 6 { i - 1 } else { i };
                                widths[index] += b.delta()?;
                            }
                        }
                    }
                }
                if b.bit()? {
                    controls += 1;
                    let width = widths[0] + if b.bit()? { b.delta()? } else { 0 };
                    let delta = b.read(3 * width, true)?;
                    if !separate {
                        last_price = add(last_price, delta)?;
                    }
                }
                if controls == 0 {
                    break;
                }
            } else if b.bit()? {
                if b.bit()? {
                    if b.bit()? {
                        days = b.days(&mut day)?;
                    } else {
                        widths[4] += b.delta()?;
                    }
                } else if post && b.bit()? {
                    let i = 6 + usize::from(extra && b.bit()?);
                    widths[i] += b.delta()?;
                } else {
                    widths[5] += b.delta()?;
                }
            } else {
                let i = b.read(2, false)? as usize;
                widths[i] += b.delta()?;
            }
        }
        let mut values = [0i64; 8];
        for i in 0..(6 + usize::from(post) + usize::from(extra)) {
            let signed = ((if separate { 191 } else { 185 }) >> i) & 1 != 0;
            values[i] = b.read(3 * widths[i], signed)?;
        }
        let date = advance(&mut day, days, weekdays)?;
        let prices = if separate {
            for i in 0..4 {
                previous[i] = add(previous[i], values[i])?;
            }
            [previous[0], previous[1], previous[2], previous[3]]
        } else {
            let open = add(last_price, values[0])?;
            last_price = add(open, values[3])?;
            [
                open,
                add(open, values[1])?,
                add(open, -values[2])?,
                last_price,
            ]
        };
        previous[4] = add(previous[4], values[4])?;
        out.push(Bar {
            date,
            open: scaled(prices[0], precision[0])?,
            high: scaled(prices[1], precision[0])?,
            low: scaled(prices[2], precision[0])?,
            close: scaled(prices[3], precision[0])?,
            volume: scaled(previous[4], precision[1])?,
        });
        // Amount/post-market values are consumed but are outside Quote's OHLCV contract.
        let _ = average;
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_history_matches_reference_decoder() {
        for (name, payload, expected) in [
            (
                "Shanghai",
                include_str!("fixtures/sh600519.txt"),
                include_str!("fixtures/sh600519.json"),
            ),
            (
                "Hong Kong",
                include_str!("fixtures/hk00700.txt"),
                include_str!("fixtures/hk00700.json"),
            ),
            (
                "US",
                include_str!("fixtures/usAAPL.txt"),
                include_str!("fixtures/usAAPL.json"),
            ),
        ] {
            let expected: serde_json::Value = serde_json::from_str(expected).unwrap();
            let rows = decode(payload).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                rows.len(),
                expected["count"].as_u64().unwrap() as usize,
                "{name}"
            );
            for (row, expected) in [&rows[0], &rows[rows.len() / 2], &rows[rows.len() - 1]]
                .into_iter()
                .zip(expected["samples"].as_array().unwrap())
            {
                assert_eq!(
                    row.date.to_string(),
                    &expected["date"].as_str().unwrap()[..10],
                    "{name}"
                );
                for (field, value) in [
                    ("open", row.open),
                    ("high", row.high),
                    ("low", row.low),
                    ("close", row.close),
                    ("volume", row.volume),
                ] {
                    let expected = Decimal::from_str_exact(&expected[field].to_string()).unwrap();
                    assert_eq!(value, expected, "{name} {} {field}", row.date);
                }
            }
        }
    }
    #[test]
    fn rejects_invalid_or_truncated_payload() {
        for text in ["", "abc", "!script", "K2/"] {
            assert!(decode(text).is_err());
        }
    }
    #[test]
    fn decimal_precision_and_calendar_match_protocol() {
        assert_eq!(scaled(12345, 6).unwrap().to_string(), "123.45");
        let mut day = 0;
        assert_eq!(advance(&mut day, 1, 62).unwrap().to_string(), "1990-12-20");
    }
}
