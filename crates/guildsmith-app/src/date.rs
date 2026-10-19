//! Calendar dates from the clock, without a date library.

use std::time::UNIX_EPOCH;

use crate::ports::Clock;

/// `YYYY-MM-DD` (UTC) for a count of days since 1970-01-01.
#[must_use]
pub fn civil(days: i64) -> String {
    // Howard Hinnant's days-from-civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's date (UTC) from `clock`.
#[must_use]
pub fn today(clock: &dyn Clock) -> String {
    let secs = clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// RFC 3339 (UTC, whole seconds) for a count of seconds since the Unix epoch.
#[must_use]
pub fn rfc3339(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    format!("{}T{:02}:{:02}:{:02}Z", civil(days), rem / 3600, rem % 3600 / 60, rem % 60)
}

/// RFC 3339 for `clock`'s now plus `secs`.
#[must_use]
pub fn rfc3339_after(clock: &dyn Clock, secs: u64) -> String {
    let now = clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    rfc3339(now.saturating_add(secs))
}

#[cfg(test)]
mod tests {
    use super::{civil, rfc3339};

    #[test]
    fn rfc3339_times() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(20_745 * 86_400 + 3_723), "2026-10-19T01:02:03Z");
    }

    #[test]
    fn known_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(20_745), "2026-10-19");
        assert_eq!(civil(11_016), "2000-02-29");
    }
}
