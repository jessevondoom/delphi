//! RFC 3339 UTC timestamps without a date-time dependency.

use std::time::{SystemTime, UNIX_EPOCH};

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for the current instant.
pub fn now_rfc3339() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format_unix_millis(d.as_secs() as i64, d.subsec_millis())
}

/// Civil-from-days (Howard Hinnant's algorithm), valid for the whole proleptic Gregorian calendar.
pub fn format_unix_millis(secs: i64, millis: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, m, d, rem / 3_600, rem % 3_600 / 60, rem % 60, millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(format_unix_millis(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_unix_millis(1_000_000_000, 5), "2001-09-09T01:46:40.005Z");
        assert_eq!(format_unix_millis(951_782_400, 0), "2000-02-29T00:00:00.000Z");
        assert_eq!(format_unix_millis(-1, 999), "1969-12-31T23:59:59.999Z");
    }
}
