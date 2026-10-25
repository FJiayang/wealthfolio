//! Transient write guards and local-price boundary planning.

use chrono::NaiveDate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryQueryKind {
    History,
    Latest,
}

pub type CustomProviderSnapshot = Vec<(String, String, String, bool, i32)>;

/// Optimistic validation only; never persisted as evidence of query completeness.
#[derive(Debug, Clone)]
pub struct HistoryWriteContext {
    pub asset: crate::assets::Asset,
    pub kind: HistoryQueryKind,
    pub custom_providers: CustomProviderSnapshot,
    pub canonical_context: Option<super::model::ProviderHistoryResetContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryWriteMode {
    Canonical,
    DerivedRange,
    DerivedLatest,
}

/// Scope of one atomic price write, not a persisted query receipt.
#[derive(Debug, Clone)]
pub struct HistoryWrite {
    pub identity: String,
    pub asset_id: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

/// Only extend the outer bounds. Internal gaps and empty responses are not tracked.
pub fn missing_boundary_ranges(
    start: NaiveDate,
    end: NaiveDate,
    bounds: Option<(NaiveDate, NaiveDate)>,
) -> Vec<(NaiveDate, NaiveDate)> {
    if start > end {
        return Vec::new();
    }
    let Some((first, last)) = bounds else {
        return vec![(start, end)];
    };
    let mut ranges = Vec::with_capacity(2);
    if start < first {
        if let Some(previous) = first.pred_opt() {
            ranges.push((start, end.min(previous)));
        }
    }
    if end > last {
        if let Some(next) = last.succ_opt() {
            ranges.push((start.max(next), end));
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, n).unwrap()
    }
    #[test]
    fn only_outer_boundaries_are_fetched() {
        assert_eq!(
            missing_boundary_ranges(day(1), day(30), Some((day(5), day(20)))),
            vec![(day(1), day(4)), (day(21), day(30))]
        );
        assert!(missing_boundary_ranges(day(8), day(18), Some((day(5), day(20)))).is_empty());
        assert_eq!(
            missing_boundary_ranges(day(1), day(3), Some((day(5), day(20)))),
            vec![(day(1), day(3))]
        );
        assert_eq!(
            missing_boundary_ranges(day(22), day(25), Some((day(5), day(20)))),
            vec![(day(22), day(25))]
        );
    }
    #[test]
    fn empty_local_and_invalid_ranges() {
        assert_eq!(
            missing_boundary_ranges(day(1), day(30), None),
            vec![(day(1), day(30))]
        );
        assert!(missing_boundary_ranges(day(30), day(1), None).is_empty());
    }
}
