//! Local request-attempt accounting, deliberately separate from quote sync state.
//!
//! An event records a transport attempt, not a successful response or billable
//! operation. SQLite and a remote server cannot share a transaction: a crash
//! between recording and dispatch can overcount, and storage failure can lose an
//! event. Recording is awaited once and fails open; it never retries the request.
//! Business parameters are explicitly selected by providers; credentials are never retained.

use async_trait::async_trait;
use chrono::{DateTime, Days, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use wealthfolio_market_data::{RequestContext, RequestOutcome};

use crate::errors::{Error, Result};

#[async_trait]
pub trait ProviderRequestRepository: Send + Sync {
    async fn start_event(
        &self,
        provider: &str,
        at: DateTime<Utc>,
        _context: &RequestContext,
    ) -> Result<Option<i64>> {
        self.insert_event(provider, at).await?;
        Ok(None)
    }
    async fn finish_event(&self, _id: i64, _outcome: &RequestOutcome) -> Result<()> {
        Ok(())
    }
    async fn prune(&self, _before: DateTime<Utc>) -> Result<()> {
        Ok(())
    }
    fn list_events(
        &self,
        _query: &RequestQuery,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<RequestEvent>> {
        Err(Error::Unexpected(
            "Request diagnostics are unavailable".into(),
        ))
    }
    fn diagnostics_since(&self) -> Result<DateTime<Utc>> {
        Err(Error::Unexpected(
            "Request diagnostics are unavailable".into(),
        ))
    }
    fn visit_diagnostics(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        visitor: &mut dyn FnMut(RequestEvent) -> Result<()>,
    ) -> Result<DateTime<Utc>> {
        self.visit_events(start, end, &mut |provider, at| {
            visitor(RequestEvent {
                id: 0,
                provider_id: provider.into(),
                occurred_at: at,
                context: None,
                result: None,
            })
        })
    }

    async fn insert_event(&self, provider_id: &str, occurred_at: DateTime<Utc>) -> Result<()>;

    /// Visit the half-open UTC range without retaining individual events.
    /// Returns the migration's UTC tracking-start marker, or an error if unknown.
    fn visit_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        visitor: &mut dyn FnMut(&str, DateTime<Utc>) -> Result<()>,
    ) -> Result<DateTime<Utc>>;
}

pub struct ProviderRequestRecorder {
    repository: Arc<dyn ProviderRequestRepository>,
}

impl ProviderRequestRecorder {
    pub fn new(repository: Arc<dyn ProviderRequestRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl wealthfolio_market_data::RequestRecorder for ProviderRequestRecorder {
    async fn start_request(
        &self,
        provider: &str,
        at: DateTime<Utc>,
        context: &RequestContext,
    ) -> Option<i64> {
        match self.repository.start_event(provider, at, context).await {
            Ok(id) => id,
            Err(_) => {
                log::warn!("Provider request statistics could not be recorded");
                None
            }
        }
    }
    async fn finish_request(&self, id: i64, outcome: &RequestOutcome) {
        if self.repository.finish_event(id, outcome).await.is_err() {
            log::warn!("Provider request outcome could not be recorded");
        }
    }

    async fn record_request(&self, provider_id: &str, dispatched_at: DateTime<Utc>) {
        if self
            .repository
            .insert_event(provider_id, dispatched_at)
            .await
            .is_err()
        {
            // Do not log a database error: it could contain request/user data.
            log::warn!("Provider request statistics could not be recorded");
        }
    }
}

pub const REQUEST_RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEvent {
    pub id: i64,
    pub provider_id: String,
    pub occurred_at: DateTime<Utc>,
    pub context: Option<RequestContext>,
    pub result: Option<RequestOutcome>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestQuery {
    pub provider_id: Option<String>,
    pub symbol: Option<String>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub record_kind: Option<String>,
    pub outcome: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPage {
    pub rows: Vec<RequestEvent>,
    pub next_cursor: Option<String>,
    pub available_since: DateTime<Utc>,
    pub diagnostics_since: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestCountRow {
    pub provider_id: String,
    pub day: NaiveDate,
    pub request_count: u64,
    pub record_kind: String,
    pub success_count: u64,
    pub failure_count: u64,
    pub unknown_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRequestCounts {
    pub timezone: String,
    pub tracking_since: DateTime<Utc>,
    pub available_since: DateTime<Utc>,
    pub as_of: DateTime<Utc>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub rows: Vec<ProviderRequestCountRow>,
}

pub struct ProviderRequestService {
    repository: Arc<dyn ProviderRequestRepository>,
}

impl ProviderRequestService {
    pub fn new(repository: Arc<dyn ProviderRequestRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_requests(&self, query: RequestQuery) -> Result<RequestPage> {
        let now = Utc::now();
        let available_since = now - Duration::days(REQUEST_RETENTION_DAYS);
        let start = query
            .start
            .unwrap_or(now - Duration::days(1))
            .max(available_since);
        let end = query.end.unwrap_or(now).min(now);
        if start >= end
            || !(1..=200).contains(&query.limit.unwrap_or(50))
            || query
                .record_kind
                .as_deref()
                .is_some_and(|v| !["http", "sdk"].contains(&v))
            || query
                .outcome
                .as_deref()
                .is_some_and(|v| !["success", "failure", "unknown"].contains(&v))
        {
            return Err(Error::InvalidConfigValue(
                "Invalid request diagnostics query".into(),
            ));
        }
        if let Some(cursor) = &query.cursor {
            parse_request_cursor(cursor)?;
        }
        self.repository.prune(available_since).await?;
        let repository = self.repository.clone();
        tokio::task::spawn_blocking(move || {
            let diagnostics_since = repository.diagnostics_since()?;
            let mut rows = repository.list_events(&query, start, end)?;
            let limit = query.limit.unwrap_or(50) as usize;
            let more = rows.len() > limit;
            rows.truncate(limit);
            let next_cursor = if more {
                rows.last()
                    .map(|row| format!("{}|{}", row.occurred_at.to_rfc3339(), row.id))
            } else {
                None
            };
            Ok(RequestPage {
                rows,
                next_cursor,
                available_since,
                diagnostics_since,
            })
        })
        .await
        .map_err(|_| Error::Unexpected("Request diagnostics task failed".into()))?
    }

    pub async fn get_counts(&self, days: u32, timezone: &str) -> Result<ProviderRequestCounts> {
        let timezone = parse_timezone(timezone)?;
        validate_days(days)?;
        let repository = self.repository.clone();
        let as_of = Utc::now();
        repository
            .prune(as_of - Duration::days(REQUEST_RETENTION_DAYS))
            .await?;
        tokio::task::spawn_blocking(move || aggregate(repository.as_ref(), days, timezone, as_of))
            .await
            .map_err(|_| Error::Unexpected("Provider request statistics task failed".into()))?
    }
}

pub fn parse_request_cursor(value: &str) -> Result<(DateTime<Utc>, i64)> {
    let invalid = || Error::InvalidConfigValue("Invalid request cursor".into());
    let (time, id) = value.rsplit_once('|').ok_or_else(invalid)?;
    let time = DateTime::parse_from_rfc3339(time)
        .map_err(|_| invalid())?
        .with_timezone(&Utc);
    let id = id.parse::<i64>().map_err(|_| invalid())?;
    if id < 1 {
        return Err(invalid());
    }
    Ok((time, id))
}

pub fn parse_timezone(timezone: &str) -> Result<Tz> {
    timezone.parse().map_err(|_| {
        Error::InvalidConfigValue("Provider request timezone must be an IANA timezone".into())
    })
}

fn validate_days(days: u32) -> Result<()> {
    if !(1..=366).contains(&days) {
        return Err(Error::InvalidConfigValue(
            "Request statistics range must be 1 to 366 days".into(),
        ));
    }
    Ok(())
}

/// Earliest instant belonging to this local date, including zones whose clock
/// moves at midnight. A fully skipped civil day maps to the following day.
fn local_day_start(date: NaiveDate, timezone: Tz) -> Result<DateTime<Utc>> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| Error::Unexpected("Invalid local date".into()))?;
    for seconds in 0..=172_800 {
        let local = midnight + Duration::seconds(seconds);
        match timezone.from_local_datetime(&local) {
            LocalResult::Single(time) => return Ok(time.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, second) => {
                return Ok(first.min(second).with_timezone(&Utc))
            }
            LocalResult::None => {}
        }
    }
    Err(Error::InvalidConfigValue(
        "Cannot resolve statistics date in timezone".into(),
    ))
}

fn aggregate(
    repository: &dyn ProviderRequestRepository,
    days: u32,
    timezone: Tz,
    as_of: DateTime<Utc>,
) -> Result<ProviderRequestCounts> {
    validate_days(days)?;
    let end_date = as_of.with_timezone(&timezone).date_naive();
    let start_date = end_date
        .checked_sub_days(Days::new(u64::from(days - 1)))
        .ok_or_else(|| Error::Unexpected("Statistics date range overflow".into()))?;
    let available_since = as_of - Duration::days(REQUEST_RETENTION_DAYS);
    let start = local_day_start(start_date, timezone)?.max(available_since);
    let mut counts: BTreeMap<(NaiveDate, String, String), (u64, u64, u64)> = BTreeMap::new();
    let tracking_since = repository.visit_diagnostics(start, as_of, &mut |event| {
        let day = event.occurred_at.with_timezone(&timezone).date_naive();
        let kind = event
            .context
            .as_ref()
            .map(|v| v.record_kind.as_str())
            .unwrap_or("http");
        let row = counts
            .entry((day, event.provider_id, kind.into()))
            .or_default();
        match event.result.as_ref().map(|v| v.outcome.as_str()) {
            Some("success") => row.0 += 1,
            Some("failure") => row.1 += 1,
            _ => row.2 += 1,
        }
        Ok(())
    })?;
    let rows = counts
        .into_iter()
        .map(
            |((day, provider_id, record_kind), (success_count, failure_count, unknown_count))| {
                ProviderRequestCountRow {
                    provider_id,
                    day,
                    record_kind,
                    success_count,
                    failure_count,
                    unknown_count,
                    request_count: success_count + failure_count + unknown_count,
                }
            },
        )
        .collect();
    Ok(ProviderRequestCounts {
        timezone: timezone.to_string(),
        tracking_since,
        available_since: available_since.max(tracking_since),
        as_of,
        start_date,
        end_date,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wealthfolio_market_data::RequestRecorder;

    struct MemoryRepository {
        events: Vec<(&'static str, DateTime<Utc>)>,
        fail: bool,
    }
    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }
    #[async_trait]
    impl ProviderRequestRepository for MemoryRepository {
        async fn insert_event(&self, _: &str, _: DateTime<Utc>) -> Result<()> {
            Err(Error::Repository("unavailable".into()))
        }
        fn visit_events(
            &self,
            start: DateTime<Utc>,
            end: DateTime<Utc>,
            visitor: &mut dyn FnMut(&str, DateTime<Utc>) -> Result<()>,
        ) -> Result<DateTime<Utc>> {
            if self.fail {
                return Err(Error::Repository("unknown tracking start".into()));
            }
            for (provider, time) in &self.events {
                if *time >= start && *time < end {
                    visitor(provider, *time)?;
                }
            }
            Ok(utc("2026-10-01T12:00:00Z"))
        }
    }

    #[test]
    fn request_counts_timezone_change_regroups_historical_events() {
        let repository = MemoryRepository {
            events: vec![
                ("YAHOO", utc("2026-10-23T15:59:59Z")),
                ("YAHOO", utc("2026-10-23T16:00:00Z")),
                ("CUSTOM_SCRAPER:abc", utc("2026-10-23T16:00:00Z")),
            ],
            fail: false,
        };
        let now = utc("2026-10-24T12:00:00Z");
        let utc_counts = aggregate(&repository, 7, chrono_tz::UTC, now).unwrap();
        let shanghai = aggregate(&repository, 7, chrono_tz::Asia::Shanghai, now).unwrap();
        assert_eq!(utc_counts.rows.len(), 2);
        assert_eq!(shanghai.rows.len(), 3);
        assert_eq!(shanghai.rows[0].day.to_string(), "2026-10-23");
        assert_eq!(shanghai.rows[1].day.to_string(), "2026-10-24");
        assert_eq!(shanghai.tracking_since, utc_counts.tracking_since);
    }

    #[test]
    fn request_counts_dst_days_are_not_fixed_24_hours() {
        let zone = chrono_tz::America::New_York;
        let spring = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let fall = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(
            (local_day_start(spring.succ_opt().unwrap(), zone).unwrap()
                - local_day_start(spring, zone).unwrap())
            .num_hours(),
            23
        );
        assert_eq!(
            (local_day_start(fall.succ_opt().unwrap(), zone).unwrap()
                - local_day_start(fall, zone).unwrap())
            .num_hours(),
            25
        );
        let repository = MemoryRepository {
            events: vec![
                ("YAHOO", utc("2026-11-01T05:30:00Z")),
                ("YAHOO", utc("2026-11-01T06:30:00Z")),
            ],
            fail: false,
        };
        let result = aggregate(&repository, 1, zone, utc("2026-11-02T04:59:00Z")).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].request_count, 2);
    }

    #[test]
    fn request_counts_handles_midnight_gap_and_skipped_civil_day() {
        let gap = NaiveDate::from_ymd_opt(2018, 11, 4).unwrap();
        assert_eq!(
            local_day_start(gap, chrono_tz::America::Sao_Paulo).unwrap(),
            utc("2018-11-04T03:00:00Z")
        );
        let skipped = NaiveDate::from_ymd_opt(2011, 12, 30).unwrap();
        assert_eq!(
            local_day_start(skipped, chrono_tz::Pacific::Apia).unwrap(),
            utc("2011-12-30T10:00:00Z")
        );
    }

    #[test]
    fn request_counts_snapshot_excludes_future_and_preserves_tracking_marker() {
        let now = utc("2026-10-24T12:00:00Z");
        let repository = MemoryRepository {
            events: vec![
                ("YAHOO", utc("2026-10-23T15:59:59.999999999Z")),
                ("YAHOO", utc("2026-10-23T16:00:00Z")),
                ("YAHOO", now),
                ("YAHOO", utc("2026-10-25T00:00:00Z")),
            ],
            fail: false,
        };
        let result = aggregate(&repository, 1, chrono_tz::Asia::Shanghai, now).unwrap();
        assert_eq!(result.start_date, result.end_date);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].request_count, 1);
        assert_eq!(result.tracking_since, utc("2026-10-01T12:00:00Z"));
        assert!(aggregate(&repository, 366, chrono_tz::UTC, now).is_ok());
    }

    #[tokio::test]
    async fn request_counts_validation_unknown_marker_and_fail_open() {
        let repository = Arc::new(MemoryRepository {
            events: vec![],
            fail: true,
        });
        let service = ProviderRequestService::new(repository.clone());
        assert!(service.get_counts(0, "UTC").await.is_err());
        assert!(service.get_counts(367, "UTC").await.is_err());
        assert!(service.get_counts(7, "Not/A_Zone").await.is_err());
        assert!(service.get_counts(7, "UTC").await.is_err());
        for query in [
            RequestQuery {
                limit: Some(0),
                ..Default::default()
            },
            RequestQuery {
                limit: Some(201),
                ..Default::default()
            },
            RequestQuery {
                record_kind: Some("invalid".into()),
                ..Default::default()
            },
            RequestQuery {
                outcome: Some("invalid".into()),
                ..Default::default()
            },
            RequestQuery {
                cursor: Some("invalid".into()),
                ..Default::default()
            },
            RequestQuery {
                start: Some(Utc::now() + Duration::days(1)),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                service.list_requests(query).await,
                Err(Error::InvalidConfigValue(_))
            ));
        }
        // No Result escapes the recorder and it performs no retry.
        ProviderRequestRecorder::new(repository)
            .record_request("YAHOO", Utc::now())
            .await;
    }
}
