//! Is this failure a spent quota, or only a rate limit? (`specs/002` §4.1, amendment 002-A1.)
//!
//! Rules, in order (rule 3, persistent throttling, needs history and lives with the pool state):
//! 1. an **explicit signal** (billing error, spend cap or limit, a CLI saying the quota is reached, a
//!    harness `rejected` status) is exhaustion, with the reset time parsed when the text has one;
//! 2. a **`retry-after` longer than [`LONG_RETRY_AFTER`]** is exhaustion until that time;
//! 4. anything else is not exhaustion. An unrecognised error never becomes exhaustion by guess.
//!
//! All functions are pure: the caller passes `now` (Unix seconds).

use regex::Regex;
use std::sync::OnceLock;
use std::time::Duration;

/// A wait longer than this is treated as a spent quota, not a rate limit.
pub const LONG_RETRY_AFTER: Duration = Duration::from_secs(15 * 60);

/// A reset further away than this is not believed (a spend cap resets within a month); text asking for more is ignored.
const MAX_RESET: Duration = Duration::from_secs(400 * 86_400);

/// Why a model is out of quota and, when known, when it comes back.
#[derive(Debug, Clone, PartialEq)]
pub struct Exhaustion {
    pub reason: String,
    /// Unix seconds.
    pub reset_at: Option<i64>,
    /// The provider's error text, kept so a wrong classification can be corrected from evidence.
    /// The caller redacts it before it is stored or shown.
    pub raw: String,
}

impl std::fmt::Display for Exhaustion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)?;
        if let Some(t) = self.reset_at {
            write!(f, " (resets at {})", delphi_core::time::format_unix_millis(t, 0))?;
        }
        Ok(())
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("pattern compiles"))
}

/// Go-style duration: `136h12m57s`, `45m`, `30s`, `1h30m`, fractions allowed (`1.5h`).
pub fn parse_go_duration(s: &str) -> Option<Duration> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let whole = re(&RE, r"^(?:\d+(?:\.\d+)?[hms])+$");
    if !whole.is_match(s) {
        return None;
    }
    static PART: OnceLock<Regex> = OnceLock::new();
    let mut secs = 0.0f64;
    for c in re(&PART, r"(\d+(?:\.\d+)?)([hms])").captures_iter(s) {
        let n: f64 = c[1].parse().ok()?;
        secs += n * match &c[2] {
            "h" => 3600.0,
            "m" => 60.0,
            _ => 1.0,
        };
    }
    // The text is the provider's: an absurd number must not panic `from_secs_f64`, or push a time past what is believable.
    Duration::try_from_secs_f64(secs).ok().filter(|d| *d <= MAX_RESET)
}

/// A reset time stated in prose: `Resets in 136h12m57s`, or `regain access on 2026-09-01 at 00:00 UTC`.
pub fn parse_reset(text: &str, now: i64) -> Option<i64> {
    static IN: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&IN, r"(?i)resets? in (?:about )?((?:\d+(?:\.\d+)?[hms])+)").captures(text) {
        return parse_go_duration(&c[1]).map(|d| now + d.as_secs() as i64);
    }
    static ON: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&ON, r"(?i)(?:regain access|access (?:will )?resum\w*) on (\d{4}-\d{2}-\d{2})(?: at (\d{2}:\d{2}))? UTC").captures(text) {
        let iso = format!("{}T{}:00Z", &c[1], c.get(2).map(|m| m.as_str()).unwrap_or("00:00"));
        return delphi_core::time::parse_rfc3339(&iso);
    }
    None
}

fn exhausted(reason: &str, reset_at: Option<i64>, raw: &str) -> Option<Exhaustion> {
    Some(Exhaustion { reason: reason.into(), reset_at, raw: raw.chars().take(500).collect() })
}

/// Classify an HTTP error reply. `body` is the raw response body.
pub fn classify_http(status: u16, retry_after: Option<Duration>, body: &str, now: i64) -> Option<Exhaustion> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let message = v["error"]["message"].as_str().or(v["message"].as_str()).unwrap_or("");
    let code = v["error"]["details"]["error_code"].as_str().or(v["error"]["code"].as_str()).unwrap_or("");
    let reset = parse_reset(message, now);

    // Rule 1: explicit signals.
    if status == 402 {
        return exhausted("billing error: credits or payment need attention", None, if message.is_empty() { body } else { message });
    }
    if code == "enforced_spend_limit_reached" {
        return exhausted("monthly spend cap reached", reset, message);
    }
    if status == 400 && message.starts_with("You have reached your specified") && message.contains("usage limits") {
        return exhausted("spend limit set on the account reached", reset, message);
    }
    if code == "quota_exceeded" {
        return exhausted("daily quota exceeded", reset, message);
    }
    // Rule 2: a very long wait is itself the signal.
    if let Some(ra) = retry_after.filter(|d| *d > LONG_RETRY_AFTER) {
        return exhausted(&format!("the provider asks for a {} wait", human(ra)), Some(now + ra.as_secs() as i64), if message.is_empty() { body } else { message });
    }
    None
}

/// Classify a CLI's error text (`agy`, `claude`). Ignores the CLI's own "retryable" flag.
pub fn classify_cli_error(text: &str, now: i64) -> Option<Exhaustion> {
    let lower = text.to_ascii_lowercase();
    // A reset that is already due ("Resets in 0s") says nothing is spent: it is not a reset time.
    let reset = parse_reset(text, now).filter(|r| *r > now);
    let says_quota = ["quota reached", "usage limit reached", "reached your usage limit", "hit your limit", "hit your session limit"].iter().any(|p| lower.contains(p));
    if says_quota || (lower.contains("resource_exhausted") && reset.is_some()) {
        return exhausted("quota reached", reset, text);
    }
    None
}

/// Classify a `claude` `rate_limit_event`. Only `rejected` is exhaustion; a utilization figure alone is not.
/// The live `claude` 2.1.310 sample gives epoch seconds (`tests/fixtures/quota/claude-cli-session-limit.md`); sources disagreed,
/// so a value above 10^12 is still taken as ms.
pub fn classify_rate_limit_event(status: &str, resets_at: Option<i64>, window: &str) -> Option<Exhaustion> {
    if status != "rejected" {
        return None;
    }
    let reset = resets_at.map(|t| if t > 1_000_000_000_000 { t / 1000 } else { t });
    exhausted(&format!("the subscription's {} window is used up", if window.is_empty() { "usage" } else { window }), reset, status)
}

fn human(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, s % 3600 / 60)
    } else {
        format!("{}m", s / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000;

    #[test]
    fn go_durations_parse_and_junk_does_not() {
        assert_eq!(parse_go_duration("136h12m57s"), Some(Duration::from_secs(136 * 3600 + 12 * 60 + 57)));
        assert_eq!(parse_go_duration("45m"), Some(Duration::from_secs(2700)));
        assert_eq!(parse_go_duration("1.5h"), Some(Duration::from_secs(5400)));
        assert_eq!(parse_go_duration("30s"), Some(Duration::from_secs(30)));
        for bad in ["", "soon", "12", "5d", "h"] {
            assert_eq!(parse_go_duration(bad), None, "{}", bad);
        }
    }

    #[test]
    fn a_402_is_exhaustion_without_a_reset() {
        let e = classify_http(402, None, r#"{"type":"error","error":{"type":"billing_error","message":"Your credit balance is too low."}}"#, NOW).unwrap();
        assert!(e.reason.contains("billing") && e.reset_at.is_none() && e.raw.contains("credit balance"));
    }

    #[test]
    fn the_documented_anthropic_spend_cap_reply_carries_its_reset_time() {
        let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"You have reached your API usage limits: your organization has crossed its monthly API usage threshold, set based on your organization's API tier. You will regain access on 2026-09-01 at 00:00 UTC.","details":{"error_code":"enforced_spend_limit_reached"}},"request_id":"req_x"}"#;
        let e = classify_http(429, None, body, NOW).unwrap();
        assert!(e.reason.contains("spend cap"));
        assert_eq!(e.reset_at, delphi_core::time::parse_rfc3339("2026-09-01T00:00:00Z"));
    }

    #[test]
    fn a_user_set_spend_limit_is_exhaustion_by_its_message() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"You have reached your specified API usage limits. You will regain access on 2026-10-01 at 00:00 UTC."}}"#;
        let e = classify_http(400, None, body, NOW).unwrap();
        assert!(e.reason.contains("spend limit") && e.reset_at.is_some());
        let ws = r#"{"error":{"type":"invalid_request_error","message":"You have reached your specified workspace API usage limits."}}"#;
        assert!(classify_http(400, None, ws, NOW).is_some());
        // An ordinary bad request is not exhaustion.
        assert!(classify_http(400, None, r#"{"error":{"type":"invalid_request_error","message":"messages: field required"}}"#, NOW).is_none());
    }

    #[test]
    fn a_long_retry_after_is_exhaustion_a_short_one_is_a_rate_limit() {
        let body = r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#;
        assert!(classify_http(429, Some(Duration::from_secs(30)), body, NOW).is_none());
        assert!(classify_http(429, Some(LONG_RETRY_AFTER), body, NOW).is_none(), "exactly at the threshold is still a throttle");
        let e = classify_http(429, Some(Duration::from_secs(16 * 60)), body, NOW).unwrap();
        assert_eq!(e.reset_at, Some(NOW + 16 * 60));
        assert!(classify_http(429, None, body, NOW).is_none(), "no header and no explicit signal: unknown, so a throttle");
    }

    #[test]
    fn overload_and_unrecognised_errors_are_never_exhaustion() {
        assert!(classify_http(529, None, r#"{"error":{"type":"overloaded_error","message":"Overloaded"}}"#, NOW).is_none());
        assert!(classify_http(503, None, "<html>unavailable</html>", NOW).is_none());
        assert!(classify_http(429, None, r#"{"error":{"code":429,"message":"slow down","status":"RESOURCE_EXHAUSTED"}}"#, NOW).is_none(), "the Gemini API's bare 429 cannot be told apart, so it stays a throttle");
        assert!(classify_http(500, None, "", NOW).is_none());
    }

    /// The text the live `agy` run printed with the Gemini quota used up (`tests/fixtures/quota/`).
    const AGY: &str = "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 136h12m54s.";

    #[test]
    fn the_real_agy_gemini_message_is_exhaustion_with_a_reset_five_days_out() {
        let e = classify_cli_error(AGY, NOW).unwrap();
        assert_eq!(e.reset_at, Some(NOW + 136 * 3600 + 12 * 60 + 54));
        let full = format!("RESOURCE_EXHAUSTED (code 429): {}", AGY);
        assert_eq!(classify_cli_error(&full, NOW).unwrap().reset_at, e.reset_at);
    }

    #[test]
    fn absurd_numbers_in_a_provider_message_never_panic_and_a_reset_already_due_is_not_exhaustion() {
        let huge = format!("RESOURCE_EXHAUSTED: Resets in {}h", "9".repeat(400));
        assert_eq!(parse_go_duration(&format!("{}h", "9".repeat(400))), None, "infinity is not a duration");
        assert_eq!(parse_go_duration("99999999999999999999999h"), None, "nor is a number past u64");
        assert_eq!(parse_go_duration("40000h"), None, "nor a reset further away than is believable");
        assert!(classify_cli_error(&huge, NOW).is_none(), "an unparseable reset is no signal");
        assert_eq!(parse_reset("regain access on 2999-01-01 at 00:00 UTC", NOW).map(|t| t > NOW), Some(true), "a date is not capped here, only durations");
        // "Resets in 0s" with RESOURCE_EXHAUSTED is a rate limit that has already cleared, not a spent quota.
        assert!(classify_cli_error("RESOURCE_EXHAUSTED: slow down. Resets in 0s.", NOW).is_none());
        assert!(classify_cli_error("RESOURCE_EXHAUSTED: slow down. Resets in 45s.", NOW).is_some(), "but a real wait still is");
        // A message that says the quota is reached still is, whatever its reset says.
        assert!(classify_cli_error("Individual quota reached. Resets in 0s.", NOW).unwrap().reset_at.is_none());
    }

    #[test]
    fn cli_errors_that_are_not_quota_are_left_alone() {
        assert!(classify_cli_error("RESOURCE_EXHAUSTED: too many requests, slow down", NOW).is_none(), "no reset and no quota wording");
        assert!(classify_cli_error("permission denied", NOW).is_none());
        assert!(classify_cli_error("", NOW).is_none());
        assert!(classify_cli_error("Claude usage limit reached. Your limit will reset at 1pm", NOW).unwrap().reset_at.is_none());
    }

    /// The text the live `claude` run printed with the session limit reached (`tests/fixtures/quota/`).
    const CLAUDE: &str = "You've hit your session limit · resets 2pm (America/New_York)";

    #[test]
    fn the_real_claude_session_limit_text_is_exhaustion_without_a_parsed_reset() {
        let e = classify_cli_error(CLAUDE, NOW).unwrap();
        assert!(e.reset_at.is_none(), "a local clock time is not parsed; the rate_limit_event carries the real one");
    }

    #[test]
    fn the_real_claude_rejected_event_is_exhaustion_with_epoch_seconds() {
        let e = classify_rate_limit_event("rejected", Some(1_792_864_800), "five_hour").unwrap();
        assert_eq!(e.reset_at, Some(1_792_864_800), "2026-10-24 14:00 EDT");
        assert!(e.reason.contains("five_hour"));
    }

    #[test]
    fn only_a_rejected_rate_limit_event_is_exhaustion_and_its_unit_is_normalised() {
        assert!(classify_rate_limit_event("allowed", Some(1), "five_hour").is_none());
        assert!(classify_rate_limit_event("allowed_warning", None, "five_hour").is_none());
        assert_eq!(classify_rate_limit_event("rejected", Some(1_790_003_600), "five_hour").unwrap().reset_at, Some(1_790_003_600));
        assert_eq!(classify_rate_limit_event("rejected", Some(1_790_003_600_000), "seven_day").unwrap().reset_at, Some(1_790_003_600), "milliseconds become seconds");
        assert!(classify_rate_limit_event("rejected", None, "").unwrap().reason.contains("usage"));
    }
}
