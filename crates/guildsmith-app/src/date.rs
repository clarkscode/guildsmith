//! Calendar dates from the clock, without a date library.

use std::time::UNIX_EPOCH;

use crate::ports::Clock;

pub use guildsmith_core::time::{civil, parse_rfc3339, rfc3339};

/// Today's date (UTC) from `clock`.
#[must_use]
pub fn today(clock: &dyn Clock) -> String {
    let secs = clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    civil(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// RFC 3339 for `clock`'s now plus `secs`.
#[must_use]
pub fn rfc3339_after(clock: &dyn Clock, secs: u64) -> String {
    let now = clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    rfc3339(now.saturating_add(secs))
}

/// Seconds since the Unix epoch, from `clock`.
#[must_use]
pub fn now_secs(clock: &dyn Clock) -> i64 {
    clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::{civil, parse_rfc3339, rfc3339};

    #[test]
    fn rfc3339_parses_back() {
        assert_eq!(parse_rfc3339("2026-10-19T01:02:03Z"), Some(20_745 * 86_400 + 3_723));
        assert_eq!(parse_rfc3339("2026-10-19T03:02:03.5+02:00"), Some(20_745 * 86_400 + 3_723));
        assert_eq!(parse_rfc3339("2026-10-18T20:02:03-05:00"), Some(20_745 * 86_400 + 3_723));
        assert_eq!(parse_rfc3339("2026-10-19T01:02:03"), None, "no offset");
        assert_eq!(parse_rfc3339("yesterday"), None);
        for secs in [0u64, 951_782_400, 1_792_411_200] {
            assert_eq!(parse_rfc3339(&rfc3339(secs)), Some(i64::try_from(secs).unwrap()));
        }
    }

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
