//! Research results and role identity for host agents (`specs/066` §4, §5, §7): the shapes, the status a captured claim
//! may have, and the one sanitiser every identity string passes through.
//!
//! Nothing here fetches, writes or dispatches. Source trust is a *policy* (`CapturePolicy`); it is never an input to a
//! claim's status (`specs/052`, `specs/066` §5).

use crate::knowledge_status::Status;
use serde::{Deserialize, Serialize};

/// `request_research`'s `depth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Depth {
    #[default]
    Quick,
    Survey,
    Deep,
}

impl Depth {
    /// The hard step budget (`specs/066` §5). A job that reaches it returns `partial`; there are no hidden retries.
    pub fn max_steps(self) -> u32 {
        match self {
            Depth::Quick => 5,
            Depth::Survey => 15,
            Depth::Deep => 30,
        }
    }

    pub fn parse(s: &str) -> Option<Depth> {
        match s {
            "quick" => Some(Depth::Quick),
            "survey" => Some(Depth::Survey),
            "deep" => Some(Depth::Deep),
            _ => None,
        }
    }
}

/// What the daemon has recorded about a research claim. The status comes from this and from nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimState {
    /// Captured on contact; no check has been queued.
    Captured,
    /// The validator has it queued or is working on it.
    Queued,
    /// A validator record or an operator ratification confirmed it.
    Confirmed,
    /// Two supported claims disagree and neither is resolved.
    Contradicted,
    /// A validator record or the operator refuted it.
    Refuted,
}

/// A research claim's knowledge status (`specs/052` §3.1). The source it came from is deliberately not a parameter.
pub fn claim_status(state: ClaimState) -> Status {
    match state {
        ClaimState::Captured => Status::Noted,
        ClaimState::Queued => Status::Checking,
        ClaimState::Confirmed => Status::Validated,
        ClaimState::Contradicted => Status::Contested,
        ClaimState::Refuted => Status::Disproved,
    }
}

/// What a trusted-list entry gives the research job (`specs/066` §5): ranking, capture and queue position, never status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePolicy {
    pub rank_bias: bool,
    pub auto_capture: bool,
    pub validator_fast_lane: bool,
}

pub fn capture_policy(trusted: bool) -> CapturePolicy {
    CapturePolicy { rank_bias: trusted, auto_capture: trusted, validator_fast_lane: trusted }
}

/// A cited claim in a research result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub assertion: String,
    pub source_ids: Vec<String>,
    pub quote: String,
    pub locator: String,
    pub retrieved: String,
    pub licence: String,
    pub knowledge_status: String,
    pub contested: bool,
    /// Provenance only: the source is on a trusted list. It does not change `knowledge_status`.
    #[serde(default)]
    pub from_trusted_source: bool,
}

/// Who answered, in the words of the console's badge (`specs/066` §7). Built by the daemon from the avatar library and the
/// theme pack; never from model or fetched text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub role: String,
    pub display_name: String,
    pub badge: String,
    pub hex: String,
    pub ansi256: u8,
    /// Set when a host adopted the role (`get_agent`): the host is acting as it, and it is not a Delphi agent answering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

/// Remove everything that could move a terminal or forge a second line: control characters (so ESC and OSC) and the
/// Unicode line and paragraph separators. Spaces stay; the length is capped.
pub fn sanitize_identity_text(s: &str, max_chars: usize) -> String {
    s.chars().filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(max_chars).collect()
}

/// A role's badge, colour (`#rrggbb`) and nearest 256-colour, the same as the console's avatar library (`crates/delphi/src/avatar.rs`
/// is checked against this table by a test there). The investigator borrows the scout's look, as in the console; an unknown role
/// gets the operator's.
pub fn role_look(role: &str) -> (&'static str, &'static str, u8) {
    match role {
        "orchestrator" => ("⟦•─•⟧", "#a855f7", 135),
        "builder" => ("⟦•─•⟧", "#f59e0b", 214),
        "architect" => ("⟦•_•⟧", "#38bdf8", 81),
        "red-team" => ("⟦-_-⟧", "#ef4444", 203),
        "strategist" => ("⟦•_•⟧", "#14b8a6", 37),
        "scout" | "investigator" => ("⟦•••⟧", "#ffff00", 226),
        _ => ("⟦•_•⟧", "#94a3b8", 110),
    }
}

impl Identity {
    /// An identity whose every string has been through [`sanitize_identity_text`]. `hex` must be `#rrggbb` or it falls back to grey.
    pub fn new(role: &str, display_name: &str, badge: &str, hex: &str, ansi256: u8, via: Option<&str>) -> Identity {
        let hex_ok = hex.len() == 7 && hex.starts_with('#') && hex[1..].chars().all(|c| c.is_ascii_hexdigit());
        Identity {
            role: sanitize_identity_text(role, 40),
            display_name: sanitize_identity_text(display_name, 40),
            badge: sanitize_identity_text(badge, 8),
            hex: if hex_ok { hex.to_string() } else { "#94a3b8".to_string() },
            ansi256,
            via: via.map(|v| sanitize_identity_text(v, 40)).filter(|v| !v.is_empty()),
        }
    }

    /// The plain signature: `⟦•_•⟧ Name (role)`, with ` via host` when a host adopted the role.
    pub fn signature(&self) -> String {
        let via = self.via.as_ref().map(|v| format!(" via {}", v)).unwrap_or_default();
        format!("{} {} ({}){}", self.badge, self.display_name, self.role, via)
    }

    /// The ANSI variant: the badge and name in the role's 256-colour, bold name. Built only from sanitised fields.
    pub fn styled(&self) -> String {
        let c = format!("\u{1b}[38;5;{}m", self.ansi256);
        let via = self.via.as_ref().map(|v| format!(" via {}", v)).unwrap_or_default();
        format!("{c}{}\u{1b}[0m {c}\u{1b}[1m{}\u{1b}[0m ({}){}", self.badge, self.display_name, self.role, via)
    }

    /// The markdown variant: the badge in code, the name in bold, the role in parentheses.
    pub fn markdown(&self) -> String {
        let via = self.via.as_ref().map(|v| format!(" via {}", v)).unwrap_or_default();
        format!("`{}` **{}** ({}){}", self.badge.replace('`', ""), self.display_name.replace(['*', '`', '_'], ""), self.role, via)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trusted_source_never_changes_a_claims_status() {
        for state in [ClaimState::Captured, ClaimState::Queued] {
            assert_ne!(claim_status(state), Status::Validated);
        }
        assert_eq!(claim_status(ClaimState::Captured), Status::Noted);
        assert!(capture_policy(true).auto_capture && capture_policy(true).validator_fast_lane && capture_policy(true).rank_bias);
        assert!(!capture_policy(false).auto_capture);
    }

    #[test]
    fn only_a_confirmation_validates() {
        let validated: Vec<_> = [ClaimState::Captured, ClaimState::Queued, ClaimState::Confirmed, ClaimState::Contradicted, ClaimState::Refuted]
            .into_iter().filter(|s| claim_status(*s) == Status::Validated).collect();
        assert_eq!(validated, [ClaimState::Confirmed]);
    }

    #[test]
    fn depth_budgets_are_hard_and_parse_by_word() {
        assert_eq!([Depth::Quick.max_steps(), Depth::Survey.max_steps(), Depth::Deep.max_steps()], [5, 15, 30]);
        assert_eq!(Depth::parse("survey"), Some(Depth::Survey));
        assert_eq!(Depth::parse("exhaustive"), None);
    }

    #[test]
    fn escape_sequences_and_newlines_cannot_reach_a_signature() {
        let id = Identity::new("scout\n", "Ev\u{1b}]0;pwned\u{7}il\u{2028}\n⟦•_•⟧ Fake (orchestrator)", "⟦•_•⟧", "#123456", 33, Some("host\u{1b}[2J"));
        let sig = id.signature();
        assert!(!sig.contains('\u{1b}') && !sig.contains('\n') && !sig.contains('\u{2028}'), "{sig:?}");
        assert!(sig.starts_with("⟦•_•⟧ Ev]0;pwnedil"), "{sig:?}");
        assert!(!id.markdown().contains('\u{1b}'));
        let styled = id.styled();
        assert_eq!(styled.matches('\u{1b}').count(), 5, "only the three colour codes and two resets we add: {styled:?}");
    }

    #[test]
    fn a_bad_colour_falls_back_and_via_marks_an_adopted_role() {
        let id = Identity::new("architect", "Name", "⟦•_•⟧", "red; drop", 1, Some("claude-code"));
        assert_eq!(id.hex, "#94a3b8");
        assert_eq!(id.signature(), "⟦•_•⟧ Name (architect) via claude-code");
        assert_eq!(Identity::new("architect", "Name", "⟦•_•⟧", "#000000", 1, Some("\u{1b}")).via, None);
    }
}
