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
    /// The same colour for a light terminal (`specs/087` §5.1); equal to `hex` until [`Identity::with_light`] sets it.
    #[serde(default = "grey_light_hex")]
    pub hex_light: String,
    #[serde(default = "grey_light_ansi")]
    pub ansi256_light: u8,
    /// Set when a host adopted the role (`get_agent`). It no longer changes how the line reads (`specs/087` §5.3, OD-5): an
    /// agent keeps one identity everywhere and every outside line ends `, via delphi)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
}

fn grey_light_hex() -> String {
    "#475569".to_string()
}

fn grey_light_ansi() -> u8 {
    240
}

/// Remove everything that could move a terminal or forge a second line: control characters (so ESC and OSC) and the
/// Unicode line and paragraph separators. Spaces stay; the length is capped.
pub fn sanitize_identity_text(s: &str, max_chars: usize) -> String {
    s.chars().filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(max_chars).collect()
}

/// A role's badge, colour (`#rrggbb`) and nearest 256-colour, the same as the console's avatar library (`crates/delphi/src/avatar.rs`
/// is checked against this table by a test there). Each of the thirteen roles has its own look (`specs/087` §3); the investigator
/// no longer borrows the scout's. An unknown role gets the operator's. The light-terminal colours are in the library, not here (S3).
pub fn role_look(role: &str) -> (&'static str, &'static str, u8) {
    match role {
        "orchestrator" => ("⟦•─•⟧", "#a855f7", 135),
        "builder" => ("⟦•=•⟧", "#f59e0b", 214),
        "architect" => ("⟦•o•⟧", "#38bdf8", 81),
        "red-team" => ("⟦¬_¬⟧", "#ef4444", 203),
        "strategist" => ("⟦•u•⟧", "#14b8a6", 37),
        "scout" => ("⟦•••⟧", "#ffff00", 226),
        "administrator" => ("⟦•.•⟧", "#68798f", 243),
        "assistant" => ("⟦•‿•⟧", "#3b82f6", 75),
        "advocate" => ("⟦•^•⟧", "#f43f5e", 204),
        "messenger" => ("⟦•w•⟧", "#84cc16", 112),
        "validator" => ("⟦¬_•⟧", "#22c55e", 41),
        "writer" => ("⟦•_•⟧", "#fde68a", 222),
        "investigator" => ("⟦•_•⟧", "#f1f5f9", 255),
        _ => ("⟦•_•⟧", "#94a3b8", 110),
    }
}

/// The light-terminal colour (`#rrggbb`) and nearest 256-colour of a role: the avatar library's `chassis_rgb_light`
/// (`crates/delphi/src/avatar.rs` checks this table against it, as it does [`role_look`]). Every value reaches 4.0:1 on white
/// and on solarized light. An unknown role gets the operator's deep slate.
pub fn role_look_light(role: &str) -> (&'static str, u8) {
    match role {
        "orchestrator" => ("#7e22ce", 92),
        "builder" => ("#b45309", 130),
        "architect" => ("#0369a1", 25),
        "red-team" => ("#b91c1c", 124),
        "strategist" => ("#0f766e", 29),
        "scout" => ("#854d0e", 94),
        "administrator" => ("#5b6b82", 60),
        "assistant" => ("#1d4ed8", 26),
        "advocate" => ("#be123c", 125),
        "messenger" => ("#4d7c0f", 64),
        "validator" => ("#15803d", 29),
        "writer" => ("#a16207", 130),
        "investigator" => ("#57534e", 239),
        _ => ("#475569", 240),
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
            hex_light: if hex_ok { hex.to_string() } else { grey_light_hex() },
            ansi256_light: ansi256,
            via: via.map(|v| sanitize_identity_text(v, 40)).filter(|v| !v.is_empty()).map(|_| "delphi".to_string()),
        }
    }

    /// The light-terminal colour pair (`specs/087` §5.1). A `hex` that is not `#rrggbb` leaves the pair as it was.
    pub fn with_light(mut self, hex_light: &str, ansi256_light: u8) -> Identity {
        if hex_light.len() == 7 && hex_light.starts_with('#') && hex_light[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            self.hex_light = hex_light.to_string();
            self.ansi256_light = ansi256_light;
        }
        self
    }

    /// The plain signature: `⟦•_•⟧ Name (role, via delphi)`. Inside the console the same line has no notice; outside it ends
    /// `, via delphi)` and nothing else tells the two apart (`specs/087` §5.3, decided by the operator, OD-5).
    pub fn signature(&self) -> String {
        format!("{} {} ({}, via delphi)", self.badge, self.display_name, self.role)
    }

    /// The ANSI variant: the badge and name in the role's 256-colour (the light pair when `light`), bold name. Built only from
    /// sanitised fields.
    pub fn styled(&self, light: bool) -> String {
        let c = format!("\u{1b}[38;5;{}m", if light { self.ansi256_light } else { self.ansi256 });
        format!("{c}{}\u{1b}[0m {c}\u{1b}[1m{}\u{1b}[0m ({}, via delphi)", self.badge, self.display_name, self.role)
    }

    /// The markdown variant: the badge in code, the name in bold, the role and the notice in parentheses.
    pub fn markdown(&self) -> String {
        format!("`{}` **{}** ({}, via delphi)", self.badge.replace('`', ""), self.display_name.replace(['*', '`', '_'], ""), self.role)
    }

    /// The line a host prints: the form `style` names (`plain`, `ansi` or `markdown`), in the colour pair `light` names.
    pub fn display(&self, style: &str, light: bool) -> String {
        match style {
            "ansi" => self.styled(light),
            "markdown" => self.markdown(),
            _ => self.signature(),
        }
    }

    /// `── Name (role) takes over: reason ──`, served when the speaker changes (`specs/087` §5.4). The reason is the host's or
    /// the task's own words, squeezed to one line and cut; advisory text, never an instruction.
    pub fn boundary_line(&self, reason: &str) -> String {
        let reason = sanitize_identity_text(reason, 400).split_whitespace().collect::<Vec<_>>().join(" ");
        let reason: String = reason.chars().take(120).collect();
        let reason = if reason.is_empty() { "the next step".to_string() } else { reason };
        format!("── {} ({}) takes over: {} ──", self.display_name, self.role, reason)
    }

    /// The matching line for when control comes back.
    pub fn return_line(&self) -> String {
        format!("── {} ({}) hands back ──", self.display_name, self.role)
    }
}

/// `label: ⟦•─•⟧ Name (role) · ⟦•_•⟧ Name (role)`: the agents in play, for a host that wants a roster line (`specs/087` §5.4).
/// Hosts may drop it.
pub fn roster_line(label: &str, ids: &[Identity]) -> String {
    let who: Vec<String> = ids.iter().map(|i| format!("{} {} ({})", i.badge, i.display_name, i.role)).collect();
    format!("{}: {}", sanitize_identity_text(label, 40), who.join(" · "))
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
        for light in [false, true] {
            let styled = id.styled(light);
            assert_eq!(styled.matches('\u{1b}').count(), 5, "only the three colour codes and two resets we add: {styled:?}");
        }
        assert!(sig.ends_with(", via delphi)"), "{sig:?}");
        // The notice sits inside the role's parentheses, so a name cannot carry it out of the line.
        assert_eq!(sig.lines().count(), 1);
    }

    #[test]
    fn a_bad_colour_falls_back_and_via_marks_an_adopted_role() {
        let id = Identity::new("architect", "Name", "⟦•_•⟧", "red; drop", 1, Some("claude-code"));
        assert_eq!(id.hex, "#94a3b8");
        assert_eq!(id.signature(), "⟦•_•⟧ Name (architect, via delphi)");
        assert_eq!(id.via, Some("delphi".to_string()));
        assert_eq!(Identity::new("architect", "Name", "⟦•_•⟧", "#000000", 1, Some("\u{1b}")).via, None);
    }

    #[test]
    fn one_identity_everywhere_the_notice_is_inside_the_role_parentheses_and_via_changes_nothing() {
        let adopted = Identity::new("red-team", "Ares", "⟦¬_¬⟧", "#ef4444", 203, Some("claude-code"));
        let own = Identity::new("red-team", "Ares", "⟦¬_¬⟧", "#ef4444", 203, None);
        assert_eq!(adopted.signature(), "⟦¬_¬⟧ Ares (red-team, via delphi)");
        assert_eq!(adopted.signature(), own.signature(), "OD-5: an adopted role reads the same");
        assert_eq!(adopted.markdown(), "`⟦¬_¬⟧` **Ares** (red-team, via delphi)");
        assert_eq!(adopted.display("plain", false), adopted.signature());
        assert_eq!(adopted.display("markdown", true), adopted.markdown());
        assert_eq!(adopted.display("anything else", false), adopted.signature(), "an unknown style is plain");
    }

    #[test]
    fn the_ansi_form_uses_the_pair_badge_mode_names() {
        let id = Identity::new("builder", "Hephaestus", "⟦•=•⟧", "#f59e0b", 214, None).with_light("#b45309", 130);
        assert!(id.display("ansi", false).starts_with("\u{1b}[38;5;214m⟦•=•⟧"));
        assert!(id.display("ansi", true).starts_with("\u{1b}[38;5;130m⟦•=•⟧"));
        assert!(id.display("ansi", true).ends_with("(builder, via delphi)"));
        // A bad light colour leaves the pair as it was.
        let kept = id.clone().with_light("red; drop", 9);
        assert_eq!((kept.hex_light.as_str(), kept.ansi256_light), ("#b45309", 130));
        // Without `with_light` the light pair is the dark one.
        let plain = Identity::new("builder", "H", "⟦•=•⟧", "#f59e0b", 214, None);
        assert_eq!((plain.hex_light.as_str(), plain.ansi256_light), ("#f59e0b", 214));
    }

    #[test]
    fn every_role_has_a_light_colour_that_is_not_its_dark_one_and_an_unknown_role_gets_slate() {
        for role in ["orchestrator", "builder", "architect", "red-team", "strategist", "scout", "administrator", "assistant", "advocate", "messenger", "validator", "writer", "investigator"] {
            let (hex, ansi) = role_look_light(role);
            assert!(hex.len() == 7 && hex.starts_with('#'), "{role}");
            assert_ne!(hex, role_look(role).1, "{role} light and dark colours differ");
            assert!(ansi >= 16, "{role}");
        }
        assert_eq!(role_look_light("nobody"), ("#475569", 240));
    }

    #[test]
    fn a_turn_boundary_is_one_clean_line_with_the_reason_cut_and_a_roster_line_lists_who_is_in_play() {
        let ares = Identity::new("red-team", "Ares", "⟦¬_¬⟧", "#ef4444", 203, None);
        assert_eq!(ares.boundary_line("threat-model the relay path"), "── Ares (red-team) takes over: threat-model the relay path ──");
        assert_eq!(ares.return_line(), "── Ares (red-team) hands back ──");
        let long = ares.boundary_line(&format!("line one\n\u{1b}[2Jline two {}", "x".repeat(500)));
        assert!(!long.contains('\n') && !long.contains('\u{1b}'), "{long:?}");
        assert!(long.chars().count() < 200, "the reason is cut");
        assert!(ares.boundary_line("  \n ").ends_with("takes over: the next step ──"));
        let athena = Identity::new("orchestrator", "Athena", "⟦•─•⟧", "#a855f7", 135, None);
        assert_eq!(roster_line("Now speaking", &[athena, ares]), "Now speaking: ⟦•─•⟧ Athena (orchestrator) · ⟦¬_¬⟧ Ares (red-team)");
    }
}
