//! The dependency policy keeps its promises (spec 093 P). `cargo deny` itself runs in CI; this checks what it cannot: that an
//! exception to the advisory check carries a reason and a date to look at it again, that the date has not passed, and that the
//! policy has not been loosened to make a failure go away.

use std::path::Path;

fn policy() -> toml::Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deny.toml");
    toml::from_str(&std::fs::read_to_string(p).expect("deny.toml exists at the repository root")).expect("deny.toml is TOML")
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The `YYYY-MM-DD` a reason ends with after `revisit by `, as days since the epoch.
fn revisit_day(reason: &str) -> Option<i64> {
    let date = reason.trim().rsplit_once("revisit by ")?.1;
    let mut it = date.split('-');
    let (y, m, d) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && it.next().is_none()).then(|| days_from_civil(y, m, d))
}

fn today() -> i64 {
    (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 86_400) as i64
}

#[test]
fn every_advisory_exception_has_a_reason_and_an_unexpired_review_date() {
    let p = policy();
    let ignores = p["advisories"]["ignore"].as_array().expect("[advisories] ignore is a list");
    let mut problems = vec![];
    for i in ignores {
        let (id, reason) = (i.get("id").and_then(|v| v.as_str()).unwrap_or(""), i.get("reason").and_then(|v| v.as_str()).unwrap_or(""));
        if !id.starts_with("RUSTSEC-") {
            problems.push(format!("an exception needs an `id` that is a RUSTSEC id (got `{id}`)"));
        }
        match revisit_day(reason) {
            None => problems.push(format!("{id}: the reason must end with `revisit by YYYY-MM-DD`")),
            Some(day) if day < today() => problems.push(format!("{id}: its review date has passed; fix the dependency or decide again and set a new date")),
            Some(_) => {}
        }
        if reason.trim().len() < 40 {
            problems.push(format!("{id}: the reason is too short to say why the advisory does not apply"));
        }
    }
    assert!(problems.is_empty(), "deny.toml exceptions:\n  {}", problems.join("\n  "));
}

#[test]
fn the_policy_has_not_been_loosened() {
    let p = policy();
    assert_eq!(p["advisories"]["yanked"].as_str(), Some("deny"));
    assert_eq!(p["sources"]["unknown-registry"].as_str(), Some("deny"));
    assert_eq!(p["sources"]["unknown-git"].as_str(), Some("deny"));
    assert!(p["sources"]["allow-git"].as_array().is_some_and(|a| a.is_empty()), "no git dependency is allowed");
    let licences: Vec<&str> = p["licenses"]["allow"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert!(!licences.is_empty() && !licences.iter().any(|l| l.contains("GPL") || l.contains("MPL") || l.contains("EUPL")), "only permissive licences are allowed: {licences:?}");
    assert!(p["graph"]["targets"].as_array().is_some_and(|t| t.len() >= 4), "the release targets are checked");
}

#[test]
fn the_date_reader_reads_dates_and_refuses_anything_else() {
    assert_eq!(revisit_day("because; revisit by 1970-01-02"), Some(1));
    assert_eq!(revisit_day("x revisit by 2000-03-01"), Some(11_017));
    assert_eq!(revisit_day("no date at all"), None);
    assert_eq!(revisit_day("revisit by 2026-13-01"), None);
    assert_eq!(revisit_day("revisit by 2026-11-07 and more"), None, "the date must end the reason");
    assert!(days_from_civil(2026, 11, 7) > days_from_civil(2026, 11, 6));
}
