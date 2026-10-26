//! UTC timestamps as seconds, for the freshness checks (spec 011 §4.1). Pure: the caller passes "now" in as
//! Unix seconds, nothing here reads a clock.

use crate::is_utc_timestamp;

/// `YYYY-MM-DDTHH:MM:SSZ` to Unix seconds. `None` unless [`is_utc_timestamp`] accepts the text and the day
/// exists in that month (the shape check alone lets `2026-02-31` through).
pub fn parse_utc(s: &str) -> Option<i64> {
    if !is_utc_timestamp(s) {
        return None;
    }
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if d > days_in_month(y, mo) {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's civil-days algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds to the 20-character form (for tests and for writing statements).
pub fn format_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants_and_round_trip() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_utc("2026-10-24T09:00:03Z"), Some(1_792_832_403));
        for s in [0i64, 951_868_800, 1_792_832_403, 4_102_444_800, 951_782_400] {
            assert_eq!(parse_utc(&format_utc(s)), Some(s));
        }
    }

    #[test]
    fn impossible_days_are_refused() {
        assert_eq!(parse_utc("2026-02-31T00:00:00Z"), None);
        assert_eq!(parse_utc("2026-02-29T00:00:00Z"), None);
        assert!(parse_utc("2028-02-29T00:00:00Z").is_some());
        assert_eq!(parse_utc("2026-10-24 09:00:03Z"), None);
        assert_eq!(parse_utc("2100-02-29T00:00:00Z"), None);
    }
}
