//! Injectable clock and local-timezone conversion (TESTING_STRATEGY §4, FR-045, FR-070).
//!
//! `SystemClock` queries the OS local time zone (`chrono::Local`).
//! `FixedClock` provides deterministic timestamps, fixed UTC offsets, and
//! simulated DST transitions without mutating process-wide environment variables.

use chrono::{DateTime, FixedOffset, Local, NaiveDate, Offset, TimeZone};

use super::time::now_ms;

pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Current time in Unix epoch milliseconds (UTC).
    fn now_ms(&self) -> i64;

    /// Local UTC offset in seconds East of UTC at the given UTC instant `epoch_ms`.
    fn utc_offset_secs(&self, epoch_ms: i64) -> i32;

    /// Converts a UTC millisecond timestamp into a local `DateTime<FixedOffset>`.
    fn to_local(&self, epoch_ms: i64) -> DateTime<FixedOffset> {
        let offset_secs = self.utc_offset_secs(epoch_ms);
        let offset = FixedOffset::east_opt(offset_secs)
            .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset valid"));
        offset
            .timestamp_millis_opt(epoch_ms)
            .single()
            .unwrap_or_else(|| {
                offset
                    .timestamp_millis_opt(0)
                    .single()
                    .expect("epoch 0 valid")
            })
    }

    /// Calendar date (`YYYY-MM-DD`) in the user's local time zone at `epoch_ms`.
    fn local_date(&self, epoch_ms: i64) -> NaiveDate {
        self.to_local(epoch_ms).date_naive()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        now_ms()
    }

    fn utc_offset_secs(&self, epoch_ms: i64) -> i32 {
        Local
            .timestamp_millis_opt(epoch_ms)
            .earliest()
            .map(|dt| dt.offset().fix().local_minus_utc())
            .unwrap_or_else(|| Local::now().offset().fix().local_minus_utc())
    }
}

#[derive(Debug, Clone)]
pub struct DstRule {
    pub transition_utc_ms: i64,
    pub before_offset_secs: i32,
    pub after_offset_secs: i32,
}

#[derive(Debug, Clone)]
pub struct FixedClock {
    pub now_ms: i64,
    pub offset_secs: i32,
    pub dst: Option<DstRule>,
}

impl FixedClock {
    pub fn utc(now_ms: i64) -> Self {
        Self {
            now_ms,
            offset_secs: 0,
            dst: None,
        }
    }

    pub fn with_offset_mins(now_ms: i64, offset_mins: i32) -> Self {
        Self {
            now_ms,
            offset_secs: offset_mins.saturating_mul(60),
            dst: None,
        }
    }

    pub fn with_dst(
        now_ms: i64,
        transition_utc_ms: i64,
        before_offset_mins: i32,
        after_offset_mins: i32,
    ) -> Self {
        Self {
            now_ms,
            offset_secs: after_offset_mins.saturating_mul(60),
            dst: Some(DstRule {
                transition_utc_ms,
                before_offset_secs: before_offset_mins.saturating_mul(60),
                after_offset_secs: after_offset_mins.saturating_mul(60),
            }),
        }
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn utc_offset_secs(&self, epoch_ms: i64) -> i32 {
        if let Some(dst) = &self.dst {
            if epoch_ms < dst.transition_utc_ms {
                dst.before_offset_secs
            } else {
                dst.after_offset_secs
            }
        } else {
            self.offset_secs
        }
    }
}

