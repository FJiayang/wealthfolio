//! Successful provider query ranges, independent of quote-date bounds.
//!
//! A sparse response does not prove every calendar day has a quote. Coverage is
//! recorded only for a successfully completed provider query, never from min/max
//! prices. Empty/partial responses are bounded retry receipts, not durable coverage;
//! failed requests never establish either.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::constants::OVERLAP_DAYS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryOutcome {
    Complete,
    Empty,
    /// Successful but bounded/truncated/partially validated provider response.
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCoverage {
    /// Digest of canonical instrument, currency and non-secret provider configuration.
    pub identity: String,
    /// Used to invalidate coverage when ordinary asset prices are deleted.
    pub asset_id: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub fetched_at: DateTime<Utc>,
    pub outcome: HistoryOutcome,
}

/// Subtract fresh successful query ranges from an inclusive requested range.
/// No assumptions are made about trading days, holidays or internal price gaps.
pub fn missing_history_ranges(
    start: NaiveDate,
    end: NaiveDate,
    coverage: &[HistoryCoverage],
    now: DateTime<Utc>,
) -> Vec<(NaiveDate, NaiveDate)> {
    missing_history_ranges_with_freshness(start, end, coverage, now, Duration::hours(6), |_| None)
}

/// Apply the caller's existing sync cadence and market-session freshness.
/// `session_freshness` returns Some(true) for a proven settled session,
/// Some(false) when a session transition requires refreshing the mutable tail,
/// and None to retain the configured cadence. Empty and partial results always use cadence and never permanent coverage.
pub fn missing_history_ranges_with_freshness(
    start: NaiveDate,
    end: NaiveDate,
    coverage: &[HistoryCoverage],
    now: DateTime<Utc>,
    retry_after: Duration,
    session_freshness: impl Fn(&HistoryCoverage) -> Option<bool>,
) -> Vec<(NaiveDate, NaiveDate)> {
    if start > end {
        return Vec::new();
    }
    let mut missing = vec![(start, end)];
    for record in coverage {
        let age = now.signed_duration_since(record.fetched_at);
        if age < Duration::zero() || record.start > record.end {
            continue;
        }
        let fresh = if record.outcome == HistoryOutcome::Complete {
            session_freshness(record).unwrap_or(age < retry_after)
        } else {
            age < retry_after
        };
        if record.outcome != HistoryOutcome::Complete && !fresh {
            continue;
        }
        // An old full-history query still covers its historical prefix. Only
        // the correction/late-NAV tail must be queried again after expiry.
        let covered_end = if fresh {
            record.end
        } else {
            record
                .end
                .min(record.fetched_at.date_naive() - Duration::days(OVERLAP_DAYS))
        };
        if record.start > covered_end {
            continue;
        }
        missing = missing
            .into_iter()
            .flat_map(|(lo, hi)| {
                if covered_end < lo || record.start > hi {
                    return vec![(lo, hi)];
                }
                let mut pieces = Vec::with_capacity(2);
                if lo < record.start {
                    if let Some(previous) = record.start.pred_opt() {
                        pieces.push((lo, previous));
                    }
                }
                if hi > covered_end {
                    if let Some(next) = covered_end.succ_opt() {
                        pieces.push((next, hi));
                    }
                }
                pieces
            })
            .collect();
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, n).unwrap()
    }

    fn record(start: u32, end: u32, now: DateTime<Utc>) -> HistoryCoverage {
        HistoryCoverage {
            identity: "canonical/config/currency".into(),
            asset_id: "asset".into(),
            start: day(start),
            end: day(end),
            fetched_at: now,
            outcome: HistoryOutcome::Complete,
        }
    }

    #[test]
    fn subtracts_overlapping_ranges_without_calendar_assumptions() {
        let now = day(30).and_hms_opt(12, 0, 0).unwrap().and_utc();
        let coverage = vec![record(4, 10, now), record(8, 15, now), record(18, 20, now)];
        assert_eq!(
            missing_history_ranges(day(1), day(25), &coverage, now),
            vec![(day(1), day(3)), (day(16), day(17)), (day(21), day(25))]
        );
    }

    #[test]
    fn stale_tail_never_becomes_covered_just_by_waiting() {
        let fetched = day(20).and_hms_opt(12, 0, 0).unwrap().and_utc();
        let coverage = vec![record(1, 20, fetched)];
        assert_eq!(
            missing_history_ranges(day(1), day(20), &coverage, fetched + Duration::days(60)),
            vec![(day(16), day(20))]
        );
    }

    #[test]
    fn settled_session_reuses_complete_ranges_but_not_empty_results() {
        let fetched = day(20).and_hms_opt(22, 0, 0).unwrap().and_utc();
        let now = fetched + Duration::days(2);
        let complete = record(1, 20, fetched);
        assert!(missing_history_ranges_with_freshness(
            day(1),
            day(20),
            std::slice::from_ref(&complete),
            now,
            Duration::hours(6),
            |_| Some(true)
        )
        .is_empty());
        let empty = HistoryCoverage {
            outcome: HistoryOutcome::Empty,
            ..complete
        };
        assert_eq!(
            missing_history_ranges_with_freshness(
                day(1),
                day(20),
                &[empty],
                now,
                Duration::hours(6),
                |_| Some(true)
            ),
            vec![(day(1), day(20))]
        );
    }

    #[test]
    fn open_session_tail_obeys_configured_retry_interval() {
        let fetched = day(20).and_hms_opt(15, 0, 0).unwrap().and_utc();
        let coverage = vec![record(1, 20, fetched)];
        let now = fetched + Duration::hours(2);
        assert!(missing_history_ranges_with_freshness(
            day(1),
            day(20),
            &coverage,
            now,
            Duration::hours(3),
            |_| None
        )
        .is_empty());
        assert_eq!(
            missing_history_ranges_with_freshness(
                day(1),
                day(20),
                &coverage,
                now,
                Duration::hours(1),
                |_| None
            ),
            vec![(day(16), day(20))]
        );
    }

    #[test]
    fn session_transition_refreshes_tail_before_cadence_but_not_empty_result() {
        let fetched = day(20).and_hms_opt(15, 0, 0).unwrap().and_utc();
        let complete = record(1, 20, fetched);
        let now = fetched + Duration::hours(1);
        assert_eq!(
            missing_history_ranges_with_freshness(
                day(1),
                day(20),
                std::slice::from_ref(&complete),
                now,
                Duration::hours(6),
                |_| Some(false)
            ),
            vec![(day(16), day(20))]
        );
        let empty = HistoryCoverage {
            outcome: HistoryOutcome::Empty,
            ..complete
        };
        assert!(missing_history_ranges_with_freshness(
            day(1),
            day(20),
            &[empty],
            now,
            Duration::hours(6),
            |_| Some(false)
        )
        .is_empty());
    }

    #[test]
    fn partial_receipt_suppresses_only_its_queried_range_until_cadence() {
        let fetched = day(20).and_hms_opt(12, 0, 0).unwrap().and_utc();
        let partial = HistoryCoverage {
            outcome: HistoryOutcome::Partial,
            ..record(5, 15, fetched)
        };
        assert_eq!(
            missing_history_ranges_with_freshness(
                day(1),
                day(20),
                std::slice::from_ref(&partial),
                fetched,
                Duration::hours(6),
                |_| Some(true)
            ),
            vec![(day(1), day(4)), (day(16), day(20))]
        );
        assert_eq!(
            missing_history_ranges_with_freshness(
                day(1),
                day(20),
                &[partial],
                fetched + Duration::hours(7),
                Duration::hours(6),
                |_| Some(true)
            ),
            vec![(day(1), day(20))]
        );
    }

    #[test]
    fn empty_and_recent_ranges_expire_but_fresh_history_is_reused() {
        let now = day(30).and_hms_opt(12, 0, 0).unwrap().and_utc();
        let mut empty = record(1, 5, now - Duration::hours(7));
        empty.outcome = HistoryOutcome::Empty;
        let coverage = vec![
            empty,
            record(6, 10, now - Duration::hours(1)),
            record(26, 30, now - Duration::hours(7)),
        ];
        assert_eq!(
            missing_history_ranges(day(1), day(30), &coverage, now),
            vec![(day(1), day(5)), (day(11), day(30))]
        );
    }
}
