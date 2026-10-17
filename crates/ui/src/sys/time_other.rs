//! Локальное время на macOS и Linux: jiff по системной zoneinfo (без вшитой базы).

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::LocalTime;

/// Пояс ОС или UTC.
#[derive(Clone, Debug)]
pub struct Zone(TimeZone);

impl Zone {
    pub fn utc() -> Self {
        Self(TimeZone::UTC)
    }

    pub fn system() -> Self {
        Self(TimeZone::system())
    }

    /// Наносекунды от Unix epoch → местное время; вне диапазона — `None`.
    pub fn local(&self, unix_ns: i64) -> Option<LocalTime> {
        let ts = Timestamp::from_nanosecond(i128::from(unix_ns)).ok()?;
        let dt = self.0.to_datetime(ts);
        Some(LocalTime {
            year: i32::from(dt.year()),
            month: dt.month().unsigned_abs(),
            day: dt.day().unsigned_abs(),
            hour: dt.hour().unsigned_abs(),
            minute: dt.minute().unsigned_abs(),
            second: dt.second().unsigned_abs(),
        })
    }
}
