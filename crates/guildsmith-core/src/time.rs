//! Calendar times without a date library: RFC 3339 to and from Unix seconds (UTC).

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

/// RFC 3339 (UTC, whole seconds) for a count of seconds since the Unix epoch.
#[must_use]
pub fn rfc3339(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    format!("{}T{:02}:{:02}:{:02}Z", civil(days), rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's days-from-civil).
#[must_use]
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch for an RFC 3339 time (`2026-10-19T12:00:00Z`,
/// `2026-10-19T12:00:00.123+02:00`). `None` if it isn't one, or has no offset.
#[must_use]
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let text = text.trim();
    let num = |from: usize, to: usize| text.get(from..to).and_then(|t| t.parse::<i64>().ok());
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if text.get(4..5)? != "-" || text.get(7..8)? != "-" || !matches!(text.get(10..11)?, "T" | "t" | " ") {
        return None;
    }
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = text.get(19..)?;
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) && &o[3..4] == ":" => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            sign * (o[1..3].parse::<i64>().ok()? * 3600 + o[4..6].parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}
