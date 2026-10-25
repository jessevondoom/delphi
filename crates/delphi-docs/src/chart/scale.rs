//! Axis arithmetic (`specs/014` §8.2): "nice" ticks, log ticks, date ticks, and number formatting. Pure functions;
//! the renderer decides what to do with them.

/// A tick step of 1, 2 or 5 times a power of ten that fits about `target` ticks into `span`.
pub fn nice_step(span: f64, target: usize) -> f64 {
    let raw = span / target.max(1) as f64;
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let r = raw / mag;
    let f = if r < 1.5 { 1.0 } else if r < 3.0 { 2.0 } else if r < 7.0 { 5.0 } else { 10.0 };
    f * mag
}

/// A linear axis covering `[min, max]`: the domain widened to whole steps, and the ticks.
pub fn linear_ticks(min: f64, max: f64, target: usize) -> (f64, f64, Vec<f64>) {
    let (mut lo, mut hi) = (min, max);
    if hi < lo {
        std::mem::swap(&mut lo, &mut hi);
    }
    if (hi - lo).abs() < f64::EPSILON * hi.abs().max(1.0) {
        // a flat series still needs a visible axis
        let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
        lo -= pad;
        hi += pad;
    }
    let step = nice_step(hi - lo, target);
    let (a, b) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    let n = ((b - a) / step).round() as usize;
    let ticks = (0..=n).map(|i| clean(a + i as f64 * step, step)).collect();
    (clean(a, step), clean(b, step), ticks)
}

/// Remove floating-point dust (0.30000000000000004) relative to the step.
fn clean(v: f64, step: f64) -> f64 {
    let digits = (-step.log10().floor()).max(0.0) as i32 + 1;
    let m = 10f64.powi(digits);
    (v * m).round() / m
}

/// Powers of ten covering `[min, max]` (both positive), plus 2 and 5 times each when the range is narrow.
pub fn log_ticks(min: f64, max: f64) -> (f64, f64, Vec<f64>) {
    let (lo_e, hi_e) = (min.log10().floor() as i32, max.log10().ceil() as i32);
    let hi_e = if hi_e == lo_e { hi_e + 1 } else { hi_e };
    let mut ticks = vec![];
    for e in lo_e..=hi_e {
        let p = 10f64.powi(e);
        ticks.push(p);
        if hi_e - lo_e <= 2 && e < hi_e {
            ticks.push(2.0 * p);
            ticks.push(5.0 * p);
        }
    }
    (10f64.powi(lo_e), 10f64.powi(hi_e), ticks)
}

// ---------------------------------------------------------------------------------------------- dates

/// Days since 1970-01-01 for a civil date (Howard Hinnant's days-from-civil).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-25`, `2026-10` or `2026` as days since the epoch (the first of the month or year).
pub fn parse_date(s: &str) -> Option<f64> {
    let t = s.trim();
    let parts: Vec<&str> = t.split('-').collect();
    let num = |p: &str, len: usize| (p.len() == len && p.chars().all(|c| c.is_ascii_digit())).then(|| p.parse::<i64>().ok()).flatten();
    let y = num(parts.first()?, 4)?;
    let m = match parts.get(1) {
        Some(p) => num(p, 2).filter(|m| (1..=12).contains(m))? as u32,
        None => 1,
    };
    let d = match parts.get(2) {
        Some(p) => num(p, 2).filter(|d| (1..=31).contains(d))? as u32,
        None => 1,
    };
    (parts.len() <= 3).then(|| days_from_civil(y, m, d) as f64)
}

/// Calendar-aligned ticks across `[lo, hi]` days, with their labels.
pub fn date_ticks(lo: f64, hi: f64) -> Vec<(f64, String)> {
    let span = (hi - lo).max(1.0);
    let (y0, m0, _) = civil_from_days(lo.floor() as i64);
    let (y1, _, _) = civil_from_days(hi.ceil() as i64);
    let mut out = vec![];
    if span >= 365.0 * 2.5 {
        let step = nice_step(span / 365.0, 6).max(1.0) as i64;
        let mut y = (y0 / step) * step;
        while y <= y1 + step {
            let d = days_from_civil(y, 1, 1) as f64;
            if d >= lo - 0.5 && d <= hi + 0.5 {
                out.push((d, y.to_string()));
            }
            y += step;
        }
    } else if span >= 75.0 {
        let months = (span / 30.4).round() as i64;
        let step = if months > 30 { 6 } else if months > 14 { 3 } else if months > 7 { 2 } else { 1 };
        let (mut y, mut m) = (y0, m0 as i64);
        m = ((m - 1) / step) * step + 1;
        for _ in 0..200 {
            let d = days_from_civil(y, m as u32, 1) as f64;
            if d > hi + 0.5 {
                break;
            }
            if d >= lo - 0.5 {
                out.push((d, format!("{:04}-{:02}", y, m)));
            }
            m += step;
            while m > 12 {
                m -= 12;
                y += 1;
            }
        }
    } else {
        let step = nice_step(span, 6).max(1.0) as i64;
        let mut d = (lo.ceil() as i64 / step) * step;
        while (d as f64) <= hi + 0.5 && out.len() < 40 {
            if (d as f64) >= lo - 0.5 {
                let (y, m, dd) = civil_from_days(d);
                out.push((d as f64, format!("{:04}-{:02}-{:02}", y, m, dd)));
            }
            d += step;
        }
    }
    out
}

pub fn date_label(days: f64) -> String {
    let (y, m, d) = civil_from_days(days.round() as i64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

// ---------------------------------------------------------------------------------------------- numbers

/// How numbers are written: `0,0 ms`, `$0,0.00`, `0.0%`. Parsed from the spec's `format`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NumFmt {
    pub prefix: String,
    pub suffix: String,
    pub thousands: bool,
    pub decimals: Option<usize>,
    pub custom: bool,
}

impl NumFmt {
    pub fn parse(spec: &str) -> NumFmt {
        let start = spec.find(['0', '#']).unwrap_or(spec.len());
        let end = spec.rfind(['0', '#']).map(|i| i + 1).unwrap_or(start);
        let core = &spec[start..end.max(start)];
        NumFmt {
            prefix: spec[..start].replace(['"', '\\'], ""),
            suffix: spec[end.max(start)..].replace(['"', '\\'], ""),
            thousands: core.contains(','),
            decimals: core.split_once('.').map(|(_, d)| d.chars().filter(|c| *c == '0' || *c == '#').count()),
            custom: true,
        }
    }

    /// A value written in full: `1,234.5`.
    pub fn full(&self, v: f64) -> String {
        let d = self.decimals.unwrap_or(if v.fract().abs() < 1e-9 { 0 } else if v.abs() < 10.0 { 2 } else { 1 });
        let s = format!("{:.*}", d, v.abs());
        let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), format!(".{}", b))).unwrap_or((s.clone(), String::new()));
        let int = if self.thousands || !self.custom { group(&int) } else { int };
        let frac = if self.decimals.is_none() { frac.trim_end_matches('0').trim_end_matches('.').to_string() } else { frac };
        format!("{}{}{}{}{}", if v < 0.0 && (int != "0" || !frac.is_empty()) { "-" } else { "" }, self.prefix, int, frac, self.suffix)
    }

    /// A value written for an axis: compact from 1,000 up (`12K`, `1.2M`) unless the format says otherwise.
    pub fn axis(&self, v: f64) -> String {
        if self.custom && (self.thousands || self.decimals.is_some()) {
            return self.full(v);
        }
        let a = v.abs();
        let (scaled, unit) = if a >= 1e9 { (v / 1e9, "B") } else if a >= 1e6 { (v / 1e6, "M") } else if a >= 1e3 { (v / 1e3, "K") } else { return self.full(v) };
        let s = format!("{:.1}", scaled);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        format!("{}{}{}{}{}", if v < 0.0 { "-" } else { "" }, self.prefix, s.trim_start_matches('-'), unit, self.suffix)
    }
}

fn group(int: &str) -> String {
    let b: Vec<char> = int.chars().collect();
    let mut out = String::new();
    for (i, c) in b.iter().enumerate() {
        if i > 0 && (b.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

/// A number for an attribute: at most two decimals, no trailing zeros, no `-0`.
pub fn f(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_clean_numbers_that_cover_the_data() {
        assert_eq!(linear_ticks(0.0, 97.0, 5), (0.0, 100.0, vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]));
        assert_eq!(linear_ticks(118.0, 340.0, 5).2, vec![100.0, 150.0, 200.0, 250.0, 300.0, 350.0]);
        let (lo, hi, t) = linear_ticks(0.0, 0.3, 5);
        assert_eq!((lo, hi), (0.0, 0.3));
        assert_eq!(t, vec![0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3]);
        let (lo, hi, _) = linear_ticks(-3.0, 8.0, 5);
        assert!(lo <= -3.0 && hi >= 8.0);
        let (lo, hi, t) = linear_ticks(5.0, 5.0, 5);
        assert!(lo < 5.0 && hi > 5.0 && t.len() >= 2, "a flat series gets a visible axis");
        let (_, _, t) = linear_ticks(0.0, 1_234_567.0, 5);
        assert!(t.iter().all(|v| (v / 200_000.0).fract().abs() < 1e-9), "{:?}", t);
    }

    #[test]
    fn log_ticks_are_powers_of_ten() {
        assert_eq!(log_ticks(3.0, 8000.0), (1.0, 10_000.0, vec![1.0, 10.0, 100.0, 1000.0, 10_000.0]));
        let (_, _, t) = log_ticks(100.0, 900.0);
        assert_eq!(t, vec![100.0, 200.0, 500.0, 1000.0]);
        assert!(log_ticks(5.0, 5.0).2.len() >= 2);
    }

    #[test]
    fn dates_parse_and_tick_on_the_calendar() {
        assert_eq!(parse_date("1970-01-01"), Some(0.0));
        assert_eq!(parse_date("2026-10-25").map(date_label).as_deref(), Some("2026-10-25"));
        assert_eq!(parse_date("2026-10").map(date_label).as_deref(), Some("2026-10-01"));
        assert_eq!(parse_date("2026").map(date_label).as_deref(), Some("2026-01-01"));
        for bad in ["2026-13-01", "26-01-01", "2026-1-1", "abc", "2026-10-25-01", "", "10/25/2026"] {
            assert_eq!(parse_date(bad), None, "{}", bad);
        }
        let (a, b) = (parse_date("2020-01-01").unwrap(), parse_date("2026-01-01").unwrap());
        let labels: Vec<String> = date_ticks(a, b).into_iter().map(|t| t.1).collect();
        assert!(labels.first().unwrap() == "2020" && labels.last().unwrap() == "2026" && labels.len() <= 8, "{:?}", labels);
        let m: Vec<String> = date_ticks(parse_date("2026-01-01").unwrap(), parse_date("2026-07-01").unwrap()).into_iter().map(|t| t.1).collect();
        assert!(m.contains(&"2026-01".to_string()) && m.contains(&"2026-04".to_string()), "{:?}", m);
        let d: Vec<String> = date_ticks(parse_date("2026-10-01").unwrap(), parse_date("2026-10-15").unwrap()).into_iter().map(|t| t.1).collect();
        assert!(d.len() >= 3 && d[0].starts_with("2026-10-"), "{:?}", d);
    }

    #[test]
    fn numbers_are_written_the_way_the_format_says() {
        let auto = NumFmt::default();
        assert_eq!((auto.full(1234.5), auto.full(0.5), auto.full(-42.0), auto.full(120.0)), ("1,234.5".into(), "0.5".into(), "-42".into(), "120".into()));
        assert_eq!((auto.axis(0.0), auto.axis(950.0), auto.axis(12_000.0), auto.axis(1_200_000.0), auto.axis(-2500.0)), ("0".into(), "950".into(), "12K".into(), "1.2M".into(), "-2.5K".into()));
        let ms = NumFmt::parse("0,0 ms");
        assert_eq!((ms.full(1234.0), ms.axis(1234.0)), ("1,234 ms".into(), "1,234 ms".into()));
        let usd = NumFmt::parse("$0,0.00");
        assert_eq!(usd.full(1234.5), "$1,234.50");
        let pct = NumFmt::parse("0.0%");
        assert_eq!(pct.full(12.345), "12.3%");
        assert_eq!(NumFmt::parse("$0").axis(2_000_000.0), "$2M");
        assert_eq!(NumFmt::parse("0\"%\"").full(76.0), "76%", "quotes mark literal text and are not printed");
        assert_eq!(NumFmt::parse("0,0 \"req/s\"").full(1200.0), "1,200 req/s");
        assert_eq!((f(1.0), f(1.2345), f(-0.001), f(10.5)), ("1".into(), "1.23".into(), "0".into(), "10.5".into()));
    }
}
