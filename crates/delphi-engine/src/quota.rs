//! Quota readings (`specs/051` §6): what a vendor's own meter says about a seat, without a model call.
//!
//! The two meters are different kinds. Codex reports a **percentage of a window** (its length and reset time, never the absolute
//! allowance); Copilot reports an **absolute pool** (and lags). So a [`Reading`] has every field optional, and a missing one is
//! *unknown*, not empty. Nothing here branches on a plan name: `free`, `business` and the rest are shown, never decided on.
//!
//! The readers are read-only: Codex's `app-server --stdio` (`account/rateLimits/read`) and, only when the operator turned it on, the
//! account endpoint behind Copilot's own CLI, read through `gh`. Neither starts a model turn.

use delphi_adapter::exhaust::Exhaustion;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// `(exit code, stdout)` of a command, or `None` when it could not be started.
pub type CliOutput = Option<(i32, String)>;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    /// The vendor's own plan label, shown and never branched on.
    pub plan: Option<String>,
    /// Share of the window or pool used, 0 to 1.
    pub used_fraction: Option<f64>,
    pub remaining: Option<f64>,
    pub entitlement: Option<f64>,
    /// What `remaining` and `entitlement` count (`credits`).
    pub unit: Option<String>,
    pub window_mins: Option<i64>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
    /// The vendor's own reason, when it says the limit is reached.
    pub reached: Option<String>,
    /// Whether going past the pool is billed to the account instead of stopping (Copilot).
    pub overage_ok: Option<bool>,
}

/// Codex's `account/rateLimits/read` result (`{ordinaryUsageAllowed, rateLimits: {primary: {usedPercent, windowDurationMins,
/// resetsAt}, planType, rateLimitReachedType, spendControlReached}}`).
pub fn codex_reading(v: &Value) -> Option<Reading> {
    let rl = v.get("rateLimits").filter(|r| r.is_object())?;
    let p = &rl["primary"];
    let reached = rl["rateLimitReachedType"].as_str().map(String::from).or_else(|| (rl["spendControlReached"] == true).then(|| "spend_control_reached".into())).or_else(|| (v["ordinaryUsageAllowed"] == false).then(|| "usage_not_allowed".into()));
    Some(Reading {
        plan: rl["planType"].as_str().map(String::from),
        used_fraction: p["usedPercent"].as_f64().map(|x| (x / 100.0).clamp(0.0, 1.0)),
        window_mins: p["windowDurationMins"].as_i64(),
        resets_at: p["resetsAt"].as_i64(),
        reached,
        ..Default::default()
    })
}

/// The account endpoint behind Copilot's own CLI (`quota_snapshots.premium_interactions`, `copilot_plan`, `quota_reset_date_utc`).
/// An `unlimited` pool has no fraction and no remainder.
pub fn copilot_reading(v: &Value) -> Option<Reading> {
    let q = v.get("quota_snapshots")?.get("premium_interactions").filter(|q| q.is_object())?;
    let unlimited = q["unlimited"] == true;
    let (entitlement, remaining) = (q["entitlement"].as_f64(), q["remaining"].as_f64());
    let used_fraction = if unlimited {
        None
    } else {
        q["percent_remaining"].as_f64().map(|p| (1.0 - p / 100.0).clamp(0.0, 1.0)).or_else(|| entitlement.zip(remaining).filter(|(e, _)| *e > 0.0).map(|(e, r)| (1.0 - r / e).clamp(0.0, 1.0)))
    };
    let overage_ok = q["overage_permitted"].as_bool();
    let reached = (!unlimited && remaining.is_some_and(|r| r <= 0.0) && entitlement.is_some_and(|e| e > 0.0)).then(|| "pool_used_up".to_string());
    Some(Reading {
        plan: v["copilot_plan"].as_str().map(String::from),
        used_fraction,
        remaining: if unlimited { None } else { remaining },
        entitlement: if unlimited { None } else { entitlement },
        unit: (!unlimited).then(|| "credits".to_string()),
        resets_at: v["quota_reset_date_utc"].as_str().or_else(|| v["quota_reset_date"].as_str()).and_then(parse_utc),
        reached,
        overage_ok,
        ..Default::default()
    })
}

/// `YYYY-MM-DD` with an optional `Thh:mm:ss[.fff]Z`, as Unix seconds. Offsets other than `Z` are not read (the vendor sends UTC).
fn parse_utc(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T').unwrap_or((s, "00:00:00"));
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let mut t = time.trim_end_matches('Z').split(':').map(|x| x.split('.').next().and_then(|n| n.parse::<i64>().ok()));
    let (hh, mm, ss) = (t.next().flatten().unwrap_or(0), t.next().flatten().unwrap_or(0), t.next().flatten().unwrap_or(0));
    // Days from the civil date (Howard Hinnant's algorithm), then the time of day.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// One line for `delphi doctor`: what the meter says, and what it does not.
pub fn describe(r: &Reading, now: i64) -> String {
    let mut parts = Vec::new();
    parts.push(match &r.plan {
        Some(p) => format!("{} plan", p.replace('_', " ")),
        None => "plan not reported".into(),
    });
    match (r.used_fraction, r.entitlement, r.remaining, r.window_mins) {
        (Some(u), Some(e), Some(rem), _) => parts.push(format!("{:.0}% of {:.0} {} used ({:.0} left)", u * 100.0, e, r.unit.as_deref().unwrap_or("units"), rem)),
        (Some(u), _, _, Some(w)) => parts.push(format!("{:.0}% of a {}-day window used (the allowance itself is not disclosed)", u * 100.0, (w as f64 / 1440.0).round())),
        (Some(u), ..) => parts.push(format!("{:.0}% used", u * 100.0)),
        (None, ..) => parts.push("no usage figure (unlimited, or not reported)".into()),
    }
    if let Some(t) = r.resets_at {
        parts.push(if t > now { format!("resets in {}", crate::pool::fmt_secs(t - now)) } else { "reset time has passed".into() });
    }
    match r.overage_ok {
        Some(true) => parts.push("going past the pool is billed, not stopped".into()),
        Some(false) => parts.push("the pool is a hard limit".into()),
        None => {}
    }
    if let Some(why) = &r.reached {
        parts.push(format!("LIMIT REACHED ({})", why));
    }
    parts.join("; ")
}

/// Whether the meter says Delphi should treat the seat as out. A pool that bills on overage is not out: the spend continues, and
/// whether that is acceptable is the operator's, not an exhaustion.
pub fn exhaustion(r: &Reading) -> Option<Exhaustion> {
    let why = r.reached.as_ref()?;
    if r.overage_ok == Some(true) {
        return None;
    }
    Some(Exhaustion { reason: format!("the account's own meter reports its limit reached ({})", why), reset_at: r.resets_at, raw: why.clone() })
}

/// Ask `codex app-server --stdio` for the account's rate limits, with no model call and no attachment to the operator's shared
/// server. `None` if it cannot be started, does not answer within `timeout`, or answers something else.
pub fn read_codex(program: &str, timeout: Duration) -> Option<Value> {
    let mut child = Command::new(program).args(["app-server", "--stdio"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let send = |stdin: &mut std::process::ChildStdin, v: Value| writeln!(stdin, "{}", v).and_then(|_| stdin.flush());
    let mut out = None;
    if send(&mut stdin, json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "delphi", "version": env!("CARGO_PKG_VERSION")}}})).is_ok() {
        let deadline = std::time::Instant::now() + timeout;
        let mut asked = false;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(m) = rx.recv_timeout(left) else { break };
            match m["id"].as_i64() {
                Some(1) if !asked => {
                    asked = true;
                    if send(&mut stdin, json!({"method": "initialized"})).and_then(|_| send(&mut stdin, json!({"id": 2, "method": "account/rateLimits/read"}))).is_err() {
                        break;
                    }
                }
                Some(2) => {
                    out = m.get("result").cloned();
                    break;
                }
                _ => {}
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    out
}

/// The account endpoint behind Copilot's CLI, read with the operator's `gh` token (a credential Delphi does not otherwise hold, and
/// an undocumented endpoint: it is opt-in, `[harness.copilot] quota_source = "gh"`, and read-only).
pub fn read_copilot_via_gh(run: &dyn Fn(&str, &[&str]) -> CliOutput) -> Option<Value> {
    let (code, out) = run("gh", &["api", "/copilot_internal/user"])?;
    if code != 0 {
        return None;
    }
    serde_json::from_str(&out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed `account/rateLimits/read` result from `codex-cli 0.165.0` on a Free plan (`specs/051` §6.1).
    fn codex_sample() -> Value {
        serde_json::from_str(r#"{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","primary":{"usedPercent":0,"windowDurationMins":43200,"resetsAt":1795765748},"secondary":null,"credits":{"hasCredits":false,"unlimited":false,"balance":null},"spendControlReached":false,"planType":"free","rateLimitReachedType":null}}"#).unwrap()
    }

    /// A trimmed `/copilot_internal/user` body for an organization-granted Business seat.
    fn copilot_sample() -> Value {
        serde_json::from_str(r#"{"copilot_plan":"business","quota_reset_date_utc":"2026-11-01T00:00:00.000Z","quota_snapshots":{"chat":{"unlimited":true},"premium_interactions":{"entitlement":100000,"remaining":99000,"percent_remaining":99.0,"overage_permitted":true,"unlimited":false}}}"#).unwrap()
    }

    #[test]
    fn a_codex_reading_is_a_percent_of_a_window_with_no_allowance() {
        let r = codex_reading(&codex_sample()).unwrap();
        assert_eq!((r.plan.as_deref(), r.used_fraction, r.window_mins, r.resets_at), (Some("free"), Some(0.0), Some(43_200), Some(1_795_765_748)));
        assert!(r.entitlement.is_none() && r.remaining.is_none() && r.reached.is_none());
        let d = describe(&r, 1_793_000_000);
        assert!(d.contains("free plan") && d.contains("0% of a 30-day window") && d.contains("not disclosed") && d.contains("resets in"), "{}", d);
        assert!(exhaustion(&r).is_none());
        assert!(codex_reading(&json!({})).is_none() && codex_reading(&json!("x")).is_none());
    }

    #[test]
    fn codex_says_why_a_limit_is_reached_and_that_is_an_exhaustion_with_the_reset_time() {
        let mut v = codex_sample();
        v["rateLimits"]["primary"]["usedPercent"] = json!(100);
        v["rateLimits"]["rateLimitReachedType"] = json!("rate_limit_reached");
        let r = codex_reading(&v).unwrap();
        assert_eq!(r.used_fraction, Some(1.0));
        let x = exhaustion(&r).unwrap();
        assert!(x.reason.contains("rate_limit_reached") && x.reset_at == Some(1_795_765_748), "{:?}", x);
        // Spend control and a refused "ordinary usage" are the same kind of answer.
        let mut w = codex_sample();
        w["rateLimits"]["spendControlReached"] = json!(true);
        assert_eq!(codex_reading(&w).unwrap().reached.as_deref(), Some("spend_control_reached"));
        let mut z = codex_sample();
        z["ordinaryUsageAllowed"] = json!(false);
        assert_eq!(codex_reading(&z).unwrap().reached.as_deref(), Some("usage_not_allowed"));
    }

    #[test]
    fn a_copilot_reading_is_an_absolute_pool_and_a_pool_that_bills_past_its_end_is_not_an_exhaustion() {
        let r = copilot_reading(&copilot_sample()).unwrap();
        assert_eq!((r.plan.as_deref(), r.entitlement, r.remaining, r.unit.as_deref(), r.overage_ok), (Some("business"), Some(100_000.0), Some(99_000.0), Some("credits"), Some(true)));
        assert!((r.used_fraction.unwrap() - 0.01).abs() < 1e-9);
        assert_eq!(r.resets_at, Some(1_793_491_200), "2026-11-01T00:00:00Z");
        let d = describe(&r, 1_793_000_000);
        assert!(d.contains("business plan") && d.contains("1% of 100000 credits used (99000 left)") && d.contains("billed, not stopped"), "{}", d);
        let mut v = copilot_sample();
        v["quota_snapshots"]["premium_interactions"]["remaining"] = json!(0);
        v["quota_snapshots"]["premium_interactions"]["percent_remaining"] = json!(0.0);
        let spent = copilot_reading(&v).unwrap();
        assert_eq!(spent.reached.as_deref(), Some("pool_used_up"));
        assert!(exhaustion(&spent).is_none(), "overage is permitted: the spend continues, which is the operator's call");
        v["quota_snapshots"]["premium_interactions"]["overage_permitted"] = json!(false);
        assert!(exhaustion(&copilot_reading(&v).unwrap()).unwrap().reason.contains("pool_used_up"));
    }

    #[test]
    fn an_unlimited_or_missing_pool_is_unknown_not_empty() {
        let mut v = copilot_sample();
        v["quota_snapshots"]["premium_interactions"] = json!({"unlimited": true, "entitlement": 0, "remaining": 0});
        let r = copilot_reading(&v).unwrap();
        assert!(r.used_fraction.is_none() && r.remaining.is_none() && r.reached.is_none(), "{:?}", r);
        assert!(describe(&r, 0).contains("no usage figure"));
        assert!(copilot_reading(&json!({"copilot_plan": "free"})).is_none(), "no snapshot: no reading");
    }

    #[test]
    fn dates_convert_to_unix_seconds() {
        assert_eq!(parse_utc("1970-01-01"), Some(0));
        assert_eq!(parse_utc("2026-11-01T00:00:00.000Z"), Some(1_793_491_200));
        assert_eq!(parse_utc("2024-03-01T12:30:15Z"), Some(1_709_296_215));
        assert_eq!(parse_utc("2026-13-01"), None);
        assert_eq!(parse_utc("soon"), None);
    }

    #[test]
    fn the_gh_reader_returns_json_only_on_success() {
        let ok = |_: &str, a: &[&str]| -> Option<(i32, String)> {
            assert_eq!(a, ["api", "/copilot_internal/user"]);
            Some((0, copilot_sample().to_string()))
        };
        assert!(read_copilot_via_gh(&ok).is_some());
        assert!(read_copilot_via_gh(&|_: &str, _: &[&str]| Some((1, "{}".into()))).is_none());
        assert!(read_copilot_via_gh(&|_: &str, _: &[&str]| None).is_none());
        assert!(read_copilot_via_gh(&|_: &str, _: &[&str]| Some((0, "not json".into()))).is_none());
    }

    #[test]
    fn the_codex_reader_speaks_the_handshake_and_gives_up_on_silence() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let prog = d.path().join("fake-codex");
        // Answers the initialize, then the rate-limit read, the way `app-server --stdio` does; the id of each is echoed.
        let body = "#!/bin/sh\nread a\necho '{\"id\":1,\"result\":{}}'\nread b\nread c\necho '{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":{\"usedPercent\":12,\"windowDurationMins\":300,\"resetsAt\":1795000000},\"planType\":\"plus\"}}}'\n";
        std::fs::write(&prog, body).unwrap();
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        let v = read_codex(&prog.display().to_string(), Duration::from_secs(10)).expect("a result");
        let r = codex_reading(&v).unwrap();
        assert_eq!((r.plan.as_deref(), r.used_fraction, r.window_mins), (Some("plus"), Some(0.12), Some(300)));
        let silent = d.path().join("silent-codex");
        std::fs::write(&silent, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = std::time::Instant::now();
        assert!(read_codex(&silent.display().to_string(), Duration::from_millis(400)).is_none());
        assert!(t.elapsed() < Duration::from_secs(5), "it gave up at the timeout");
        assert!(read_codex("/nonexistent/codex", Duration::from_secs(1)).is_none());
    }
}
