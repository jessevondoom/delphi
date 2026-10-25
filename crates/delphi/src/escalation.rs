//! How a task that stopped and needs the operator reads in the Rust consoles (`specs/005e` §6.6). The engine's own
//! reason strings stay in the log; the console shows the stop, not the plumbing: no `[escalated to the operator: …]`
//! wrapper, and "throttled" only when a pool exists and is all down. The full-screen console has the same table in
//! `ui/src/attention.ts`; keep the two in step.

/// What to show for one stopped task: the first line, and what the operator can do about it.
#[derive(Debug, PartialEq, Eq)]
pub struct Attention {
    /// `no_model_configured`, `budget`, `reauth_required`, `exhausted` or `other`.
    pub kind: &'static str,
    pub headline: String,
    pub hints: Vec<String>,
}

/// The reason inside the engine's `[escalated to the operator: …]` wrapper, or the text itself.
pub fn strip_wrapper(text: &str) -> &str {
    let t = text.trim();
    t.strip_prefix("[escalated to the operator:").and_then(|r| r.strip_suffix(']')).map(str::trim).unwrap_or(t)
}

pub fn classify(text: &str) -> Attention {
    let reason = strip_wrapper(text);
    let hints = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    if reason.contains("no_model_configured") {
        Attention {
            kind: "no_model_configured",
            headline: "Stopped: no model is configured for this workspace.".into(),
            hints: hints(&["delphi setup     connects one", "/models          shows the pool and what is usable"]),
        }
    } else if reason.starts_with("mission budget") {
        Attention {
            kind: "budget",
            headline: "Stopped: this mission's ceiling is reached.".into(),
            hints: hints(&["/budget <dollars>   raises it", "/usage              shows where it went"]),
        }
    } else if let Some(endpoint) = reason.strip_prefix("re-authenticate ") {
        Attention {
            kind: "reauth_required",
            headline: format!("Stopped: {} needs you to sign in again.", endpoint.trim()),
            hints: hints(&["aws sso login (or the provider's command), then send again"]),
        }
    } else if reason.starts_with("every allowed endpoint is throttled or unavailable") {
        Attention { kind: "exhausted", headline: "Stopped: every usable model is out of quota or down.".into(), hints: hints(&["/models shows when each is back"]) }
    } else {
        Attention { kind: "other", headline: format!("Stopped: {}", reason), hints: hints(&["/resume to try again · /models · /usage"]) }
    }
}

impl Attention {
    /// The headline, a blank line, then the hints: the body of the card.
    pub fn body(&self) -> String {
        format!("{}\n\n{}", self.headline, self.hints.iter().map(|h| format!("  {}", h)).collect::<Vec<_>>().join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wrapper_is_stripped_and_the_known_stops_get_their_copy() {
        let a = classify("[escalated to the operator: no_model_configured: no model is configured for this workspace]");
        assert_eq!((a.kind, a.headline.as_str()), ("no_model_configured", "Stopped: no model is configured for this workspace."));
        assert!(a.body().contains("  delphi setup     connects one"));
        assert_eq!(classify("[escalated to the operator: mission budget: this call could cost up to $1 but only $0 remains (raise it with /budget)]").kind, "budget");
        assert_eq!(classify("re-authenticate corp-gw").headline, "Stopped: corp-gw needs you to sign in again.");
        assert_eq!(classify("every allowed endpoint is throttled or unavailable: x").kind, "exhausted");
        let other = classify("[escalated to the operator: the same turn was interrupted 3 times in a row]");
        assert_eq!((other.kind, other.headline.as_str()), ("other", "Stopped: the same turn was interrupted 3 times in a row"));
        assert_eq!(strip_wrapper("plain"), "plain");
    }

    #[test]
    fn nothing_the_operator_reads_says_throttled_for_a_workspace_with_no_model() {
        let a = classify("[escalated to the operator: no_model_configured: no model is configured for this workspace]");
        assert!(!a.body().contains("throttled") && !a.body().contains("[escalated"));
    }
}
