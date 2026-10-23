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

/// Parse an RFC 3339 timestamp (`Z` or a numeric offset, optional fraction) into Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b't' || b[10] == b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse::<i64>().ok() };
    let (y, mo, d, h, mi, sec) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    let offset = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        b'+' | b'-' if i + 6 == b.len() && b[i + 3] == b':' => {
            let sign = if b[i] == b'+' { 1 } else { -1 };
            sign * (num(i + 1..i + 3)? * 3600 + num(i + 4..i + 6)? * 60)
        }
        _ => return None,
    };
    // days-from-civil (Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + mi * 60 + sec - offset)
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

    #[test]
    fn parse_roundtrips_and_handles_offsets() {
        for secs in [0i64, 1_000_000_000, 951_782_400, 1_790_000_123, -86_400] {
            assert_eq!(parse_rfc3339(&format_unix_millis(secs, 0)), Some(secs));
        }
        assert_eq!(parse_rfc3339("2026-10-23T12:00:00+02:00"), parse_rfc3339("2026-10-23T10:00:00Z"));
        assert_eq!(parse_rfc3339("2026-10-23T12:00:00.250Z"), parse_rfc3339("2026-10-23T12:00:00Z"));
        for bad in ["", "yesterday", "2026-13-01T00:00:00Z", "2026-10-23T25:00:00Z", "2026-10-23T12:00:00", "2026-10-23T12:00:00+0200"] {
            assert_eq!(parse_rfc3339(bad), None, "{}", bad);
        }
    }
}
