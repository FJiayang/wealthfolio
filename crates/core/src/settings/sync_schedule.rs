//! Pure due-time calculation for the existing automatic market-data task.
//! Records describe attempts, not quote completeness, success, or freshness.
use chrono::{DateTime, Duration, LocalResult, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::{MarketDataSyncMode, Settings};
use crate::{errors::Result, Error};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum MarketDataSyncSchedule {
    Interval { hours: u32 },
    Daily { time: NaiveTime, timezone: String },
}

fn invalid(message: &str) -> Error {
    Error::InvalidConfigValue(message.to_string())
}

pub fn validate_market_data_sync_time(value: &str) -> Result<NaiveTime> {
    let parsed = NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| invalid("Invalid market data sync time; expected HH:mm"))?;
    if parsed.format("%H:%M").to_string() != value {
        return Err(invalid("Invalid market data sync time; expected HH:mm"));
    }
    Ok(parsed)
}

pub fn validate_market_data_sync_timezone(value: &str) -> Result<Tz> {
    value
        .parse()
        .map_err(|_| invalid("Invalid market data sync timezone"))
}

impl MarketDataSyncSchedule {
    pub fn from_settings(settings: &Settings) -> Result<Self> {
        if !matches!(settings.market_data_sync_interval_hours, 1 | 6 | 12 | 24) {
            return Err(invalid("Invalid market data sync interval"));
        }
        let time = settings
            .market_data_sync_time
            .as_deref()
            .map(validate_market_data_sync_time)
            .transpose()?;
        let timezone = settings
            .market_data_sync_timezone
            .as_deref()
            .map(validate_market_data_sync_timezone)
            .transpose()?;
        match settings.market_data_sync_mode {
            MarketDataSyncMode::Interval => Ok(Self::Interval {
                hours: settings.market_data_sync_interval_hours,
            }),
            MarketDataSyncMode::Daily => Ok(Self::Daily {
                time: time.ok_or_else(|| invalid("Daily market sync requires a time"))?,
                timezone: timezone
                    .ok_or_else(|| invalid("Daily market sync requires a timezone"))?
                    .name()
                    .to_string(),
            }),
        }
    }

    /// Strictly after the baseline: changing settings never replays the old plan.
    pub fn next_after(&self, baseline: DateTime<Utc>) -> Result<DateTime<Utc>> {
        match self {
            Self::Interval { hours } => baseline
                .checked_add_signed(Duration::hours(i64::from(*hours)))
                .ok_or_else(|| invalid("Market sync schedule is outside supported dates")),
            Self::Daily { time, timezone } => {
                let timezone = validate_market_data_sync_timezone(timezone)?;
                let mut date = baseline.with_timezone(&timezone).date_naive();
                loop {
                    let mut local = date.and_time(*time);
                    // During a gap, advance to the first valid local minute on
                    // this date. A wholly skipped date has no daily occurrence.
                    while local.date() == date {
                        let instant = match timezone.from_local_datetime(&local) {
                            LocalResult::Single(value) => Some(value.with_timezone(&Utc)),
                            LocalResult::Ambiguous(first, second) => {
                                Some(first.min(second).with_timezone(&Utc))
                            }
                            LocalResult::None => None,
                        };
                        if let Some(instant) = instant {
                            if instant > baseline {
                                return Ok(instant);
                            }
                            break;
                        }
                        local =
                            local
                                .checked_add_signed(Duration::minutes(1))
                                .ok_or_else(|| {
                                    invalid("Market sync schedule is outside supported dates")
                                })?;
                    }
                    date = date.succ_opt().ok_or_else(|| {
                        invalid("Market sync schedule is outside supported dates")
                    })?;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataAutoSyncRecord {
    pub schedule: MarketDataSyncSchedule,
    /// Configuration activation is distinct from a network attempt. It gives
    /// saves a future baseline without fabricating a successful/failed attempt.
    pub effective_since: DateTime<Utc>,
    pub last_attempt_started_at: Option<DateTime<Utc>>,
}

impl MarketDataAutoSyncRecord {
    pub fn configured(schedule: MarketDataSyncSchedule, now: DateTime<Utc>) -> Self {
        Self {
            schedule,
            effective_since: now,
            last_attempt_started_at: None,
        }
    }

    pub fn is_due(&self, now: DateTime<Utc>) -> Result<bool> {
        let attempt = self
            .last_attempt_started_at
            .filter(|last| *last >= self.effective_since);
        let baseline = attempt.unwrap_or(self.effective_since);
        let mut next = self.schedule.next_after(baseline)?;
        if let (Some(last), MarketDataSyncSchedule::Daily { timezone, .. }) =
            (attempt, &self.schedule)
        {
            let timezone = validate_market_data_sync_timezone(timezone)?;
            // A morning catch-up consumes that local day's one automatic run,
            // even when the configured wall-clock time is still ahead.
            if next.with_timezone(&timezone).date_naive()
                == last.with_timezone(&timezone).date_naive()
            {
                next = self.schedule.next_after(next)?;
            }
        }
        Ok(next <= now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }
    fn daily(time: &str, timezone: &str) -> MarketDataSyncSchedule {
        MarketDataSyncSchedule::Daily {
            time: validate_market_data_sync_time(time).unwrap(),
            timezone: timezone.to_string(),
        }
    }
    #[test]
    fn legacy_settings_default_to_interval_and_preserve_hours() {
        let settings = Settings::default();
        assert_eq!(
            MarketDataSyncSchedule::from_settings(&settings).unwrap(),
            MarketDataSyncSchedule::Interval { hours: 6 }
        );
        let mut value = serde_json::to_value(settings).unwrap();
        value.as_object_mut().unwrap().remove("marketDataSyncMode");
        value["marketDataSyncIntervalHours"] = 12.into();
        let settings = serde_json::from_value(value).unwrap();
        assert_eq!(
            MarketDataSyncSchedule::from_settings(&settings).unwrap(),
            MarketDataSyncSchedule::Interval { hours: 12 }
        );
    }
    #[test]
    fn daily_requires_explicit_valid_time_and_timezone() {
        let mut settings = Settings {
            market_data_sync_mode: MarketDataSyncMode::Daily,
            ..Settings::default()
        };
        assert!(MarketDataSyncSchedule::from_settings(&settings).is_err());
        settings.market_data_sync_time = Some("18:00".into());
        assert!(MarketDataSyncSchedule::from_settings(&settings).is_err());
        settings.market_data_sync_timezone = Some("Asia/Shanghai".into());
        assert!(MarketDataSyncSchedule::from_settings(&settings).is_ok());
        for value in ["9:30", "24:00", "12:01:00", "", " 12:00"] {
            assert!(validate_market_data_sync_time(value).is_err());
        }
        assert!(validate_market_data_sync_timezone("UTC+8").is_err());
    }
    #[test]
    fn interval_due_is_based_on_start_and_config_changes_start_in_future() {
        let start = at("2026-10-25T00:00:00Z");
        let mut record = MarketDataAutoSyncRecord::configured(
            MarketDataSyncSchedule::Interval { hours: 6 },
            start,
        );
        record.last_attempt_started_at = Some(start);
        assert!(!record
            .is_due(start + Duration::hours(6) - Duration::seconds(1))
            .unwrap());
        assert!(record.is_due(start + Duration::hours(6)).unwrap());
        let changed = MarketDataAutoSyncRecord::configured(
            MarketDataSyncSchedule::Interval { hours: 1 },
            start + Duration::hours(4),
        );
        assert!(!changed.is_due(start + Duration::hours(4)).unwrap());
        assert!(changed.is_due(start + Duration::hours(5)).unwrap());
    }
    #[test]
    fn daily_uses_independent_timezone_and_never_skips_weekends() {
        let schedule = daily("16:00", "Asia/Shanghai");
        assert_eq!(
            schedule.next_after(at("2026-10-25T07:59:59Z")).unwrap(),
            at("2026-10-25T08:00:00Z")
        );
        assert_eq!(
            schedule.next_after(at("2026-10-25T08:00:00Z")).unwrap(),
            at("2026-10-26T08:00:00Z")
        );
    }
    #[test]
    fn daylight_saving_duplicate_uses_first_occurrence_only() {
        let schedule = daily("01:30", "America/New_York");
        assert_eq!(
            schedule.next_after(at("2026-11-01T04:00:00Z")).unwrap(),
            at("2026-11-01T05:30:00Z")
        );
        assert_eq!(
            schedule.next_after(at("2026-11-01T05:30:00Z")).unwrap(),
            at("2026-11-02T06:30:00Z")
        );
    }
    #[test]
    fn daylight_saving_gap_advances_to_first_valid_time() {
        let schedule = daily("02:30", "America/New_York");
        assert_eq!(
            schedule.next_after(at("2026-03-08T05:00:00Z")).unwrap(),
            at("2026-03-08T07:00:00Z")
        );
    }
    #[test]
    fn missed_days_and_failed_attempts_do_not_replay() {
        let mut record = MarketDataAutoSyncRecord::configured(
            daily("16:00", "Asia/Shanghai"),
            at("2026-10-20T00:00:00Z"),
        );
        let resumed = at("2026-10-25T12:00:00Z");
        assert!(record.is_due(resumed).unwrap());
        record.last_attempt_started_at = Some(resumed);
        // No success flag: even failure/crash consumes this attempt.
        assert!(!record.is_due(resumed + Duration::minutes(1)).unwrap());
        assert!(record.is_due(at("2026-10-26T08:00:00Z")).unwrap());
        let restored: MarketDataAutoSyncRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert!(!restored.is_due(resumed).unwrap());
    }
    #[test]
    fn morning_catchup_consumes_local_day_even_before_scheduled_time() {
        let mut record = MarketDataAutoSyncRecord::configured(
            daily("16:00", "Asia/Shanghai"),
            at("2026-10-20T00:00:00Z"),
        );
        let resumed = at("2026-10-25T02:00:00Z");
        assert!(record.is_due(resumed).unwrap());
        record.last_attempt_started_at = Some(resumed);
        assert!(!record.is_due(at("2026-10-25T08:00:00Z")).unwrap());
        assert!(record.is_due(at("2026-10-26T08:00:00Z")).unwrap());
    }
    #[test]
    fn changing_plan_preserves_actual_attempt_without_suppressing_new_daily_slot() {
        let mut record = MarketDataAutoSyncRecord::configured(
            daily("16:00", "Asia/Shanghai"),
            at("2026-10-25T03:00:00Z"),
        );
        record.last_attempt_started_at = Some(at("2026-10-25T02:00:00Z"));
        assert!(!record.is_due(at("2026-10-25T03:00:00Z")).unwrap());
        assert!(record.is_due(at("2026-10-25T08:00:00Z")).unwrap());
    }
}
