//! Provenance and taint (`specs/001` §5).
//!
//! Trust is a property of *content*, never of what a file says about itself. The daemon keeps an
//! index `(zone, path, blake3) → tier`, written as `provenance.stamp` events so it is rebuildable.
//! A span's `external_ceiling` starts at `L5` and only goes down, when external or federated
//! (`≤ L2`) input enters its context. Taint-gated actions (§5.2) are decided from that ceiling, not
//! from anything the model says.

use crate::event::EventEnvelope;
use crate::pathguard::glob_match;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// `external_untrusted`
    L0,
    /// `federated_peer`
    L1,
    /// `federated_verified`
    L2,
    /// `internal_generated`
    L3,
    /// `internal_empirical`
    L4,
    /// `user_ratified`
    L5,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::L0 => "L0",
            Tier::L1 => "L1",
            Tier::L2 => "L2",
            Tier::L3 => "L3",
            Tier::L4 => "L4",
            Tier::L5 => "L5",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tier::L0 => "external_untrusted",
            Tier::L1 => "federated_peer",
            Tier::L2 => "federated_verified",
            Tier::L3 => "internal_generated",
            Tier::L4 => "internal_empirical",
            Tier::L5 => "user_ratified",
        }
    }

    pub fn parse(s: &str) -> Option<Tier> {
        Some(match s {
            "L0" => Tier::L0,
            "L1" => Tier::L1,
            "L2" => Tier::L2,
            "L3" => Tier::L3,
            "L4" => Tier::L4,
            "L5" => Tier::L5,
            _ => return None,
        })
    }

    /// External or federated input: the only kind that lowers a span's ceiling.
    pub fn is_external(&self) -> bool {
        *self <= Tier::L2
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Where a file lives, for origin defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Seeded release files (`base/`), ratified by installing a verified build.
    Release,
    /// `team/` content.
    Team,
    /// The operator's private zone (`local/`).
    Local,
    /// A registered project repo (`@project/**`).
    Project,
    /// A task worktree.
    Worktree,
    /// An added org bundle (`@bundle/<org>/<name>/…`): federated content the operator chose to add (`specs/024` §3).
    Bundle,
    /// A Xenia record in the knowledge-base zone (`specs/011` §5, milestone X6). Its tier is computed by the daemon on
    /// every sync and stamped per content hash; with no stamp it is the lowest tier, never a default of the zone.
    Xenia,
}

impl Zone {
    pub fn as_str(&self) -> &'static str {
        match self {
            Zone::Release => "release",
            Zone::Team => "team",
            Zone::Local => "local",
            Zone::Project => "project",
            Zone::Worktree => "worktree",
            Zone::Bundle => "bundle",
            Zone::Xenia => "xenia",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OriginRules {
    /// `untrusted_paths` from `config.toml` (default `vendor/**`, `third_party/**`, `node_modules/**`).
    pub untrusted_paths: Vec<String>,
}

impl OriginRules {
    pub fn with_defaults() -> OriginRules {
        OriginRules { untrusted_paths: vec!["vendor/**".into(), "third_party/**".into(), "node_modules/**".into()] }
    }

    pub fn untrusted(&self, project_relative: &str) -> bool {
        self.untrusted_paths.iter().any(|g| glob_match(g, project_relative))
    }
}

/// Mission-local output globs the tainted roles (scout, investigator, writer) may write. The stamp that records a
/// span's ceiling lives only in that mission's log, so a later mission has nothing to read it from: these paths
/// default to `L0` instead (lower-only; `specs/001` amendment 001-A4).
pub const TAINTED_OUTPUT_GLOBS: &[&str] = &["missions/**/research/**", "missions/**/reports/**"];

/// The tier of a file with no index entry for its current hash.
pub fn origin_default(zone: Zone, path: &str, rules: &OriginRules, clean_at_base: bool) -> Tier {
    match zone {
        Zone::Release => Tier::L5,
        Zone::Project => {
            if rules.untrusted(path) {
                Tier::L0
            } else if clean_at_base {
                Tier::L4
            } else {
                Tier::L3 // a local edit since the base commit
            }
        }
        // The operator's add lifts a bundle to the tier `team/` content has; never higher without a per-file act.
        Zone::Team | Zone::Bundle => Tier::L2,
        // No stamp for this exact content: the record was not computed here, so it is not trusted.
        Zone::Xenia => Tier::L0,
        Zone::Local if TAINTED_OUTPUT_GLOBS.iter().any(|g| glob_match(g, path)) => Tier::L0,
        Zone::Local | Zone::Worktree => Tier::L3,
    }
}

/// `(zone, path, blake3) → tier`, rebuilt from `provenance.stamp` events.
#[derive(Debug, Default)]
pub struct Index {
    map: HashMap<(String, String, String), Tier>,
}

impl Index {
    pub fn new() -> Index {
        Index::default()
    }

    pub fn stamp(&mut self, zone: Zone, path: &str, blake3: &str, tier: Tier) {
        self.map.insert((zone.as_str().into(), path.into(), blake3.into()), tier);
    }

    pub fn lookup(&self, zone: Zone, path: &str, blake3: &str) -> Option<Tier> {
        self.map.get(&(zone.as_str().into(), path.into(), blake3.into())).copied()
    }

    /// The tier on read: the index entry for this exact content hash, else the origin default.
    /// Frontmatter is never consulted.
    pub fn tier_of(&self, zone: Zone, path: &str, blake3: &str, rules: &OriginRules, clean_at_base: bool) -> Tier {
        self.lookup(zone, path, blake3).unwrap_or_else(|| origin_default(zone, path, rules, clean_at_base))
    }

    /// Payload for the `provenance.stamp` event that records a stamp.
    pub fn stamp_payload(zone: Zone, path: &str, blake3: &str, tier: Tier) -> serde_json::Value {
        serde_json::json!({"zone": zone.as_str(), "path": path, "blake3": blake3, "tier": tier.as_str()})
    }

    /// As [`Index::stamp_payload`], with the reason the stamp was written (`bundle-add:<hash>`, `specs/024` §3.4).
    pub fn stamp_payload_with_reason(zone: Zone, path: &str, blake3: &str, tier: Tier, reason: &str) -> serde_json::Value {
        let mut p = Self::stamp_payload(zone, path, blake3, tier);
        p["reason"] = reason.into();
        p
    }

    /// Rebuild from a log: apply every `provenance.stamp` event.
    pub fn apply_event(&mut self, e: &EventEnvelope) {
        if e.kind != "provenance.stamp" {
            return;
        }
        let p = &e.payload;
        if let (Some(z), Some(path), Some(h), Some(t)) = (p["zone"].as_str(), p["path"].as_str(), p["blake3"].as_str(), p["tier"].as_str().and_then(Tier::parse)) {
            self.map.insert((z.into(), path.into(), h.into()), t);
        }
    }
}

/// How an artifact came to be, for stamping what a span writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCheck {
    /// No verification: `min(L3, ceiling)`.
    Unverified,
    /// Covered by a passing deterministic check run by the daemon: `min(L4, ceiling)`.
    Verified,
    /// Ratified by the operator: `L5`, the only path there, which also resets the artifact's ceiling.
    Ratified,
}

pub fn output_tier(ceiling: Tier, check: OutputCheck) -> Tier {
    match check {
        OutputCheck::Unverified => ceiling.min(Tier::L3),
        OutputCheck::Verified => ceiling.min(Tier::L4),
        OutputCheck::Ratified => Tier::L5,
    }
}

/// A span's taint state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub id: String,
    ceiling: Tier,
}

impl Span {
    pub fn root(id: &str) -> Span {
        Span { id: id.into(), ceiling: Tier::L5 }
    }

    /// A span restored from the log with the ceiling it had reached.
    pub fn with_ceiling(id: &str, ceiling: Tier) -> Span {
        Span { id: id.into(), ceiling }
    }

    pub fn ceiling(&self) -> Tier {
        self.ceiling
    }

    /// Account for an input that entered the span's context. Only external or federated (`≤ L2`)
    /// input lowers the ceiling; local input never does, which is why verification can raise output
    /// trust without contradiction. Returns true if the ceiling dropped.
    pub fn observe_input(&mut self, tier: Tier) -> bool {
        if tier.is_external() && tier < self.ceiling {
            self.ceiling = tier;
            true
        } else {
            false
        }
    }

    /// A child span starts at its parent's ceiling.
    pub fn child(&self, id: &str) -> Span {
        Span { id: id.into(), ceiling: self.ceiling }
    }

    pub fn output_tier(&self, check: OutputCheck) -> Tier {
        output_tier(self.ceiling, check)
    }
}

/// Logical paths that change only through ratified proposals; a span below `L4` may not write them
/// (`specs/001` §5.1 gate), whatever its manifest says.
pub fn is_gated_path(logical: &str) -> bool {
    let lower = logical.to_lowercase();
    let mut l = lower.trim_start_matches("./");
    if let Some(rest) = l.strip_prefix("@worktree/") {
        l = rest;
    } else if let Some(rest) = l.strip_prefix("@project/") {
        l = rest.split_once('/').map(|(_, r)| r).unwrap_or("");
    }
    let l = l.trim_start_matches("./");
    l == "context/heuristics.md"
        || l == "review.md"
        || l == "governance.md"
        || (l.starts_with("memory/agents/") && l.ends_with("/lessons.md"))
        || l.starts_with("agents/")
        || l.starts_with("skills/")
}

#[derive(Debug, PartialEq, Eq)]
pub struct GateDenied {
    pub path: String,
    pub ceiling: Tier,
}

impl std::fmt::Display for GateDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "provenance gate: `{}` changes only through ratified proposals, and this span's ceiling is {} (below L4)", self.path, self.ceiling)
    }
}

pub fn gate_write(logical: &str, ceiling: Tier) -> Result<(), GateDenied> {
    if is_gated_path(logical) && ceiling < Tier::L4 {
        return Err(GateDenied { path: logical.into(), ceiling });
    }
    Ok(())
}

/// An action class in the taint-gated table (`specs/001` §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    /// Worktree writes and code execution.
    WorktreeWrite,
    /// `gh`, network, agent-to-agent calls.
    Outbound,
    DispatchAgent,
    /// A write through a connector (`specs/010` §6): sends, posts, shares, invites, deletes.
    ConnectorWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Allowed, subject to the ordinary HITL tier for the specific action.
    Allow,
    /// Allowed only with a synchronous operator approval that no permission mode can waive.
    Red,
    Deny,
}

pub fn gate(ceiling: Tier, action: Action) -> Gate {
    let tainted = ceiling <= Tier::L1;
    match action {
        Action::Read => Gate::Allow,
        Action::WorktreeWrite => {
            if tainted {
                Gate::Deny
            } else {
                Gate::Allow
            }
        }
        Action::Outbound => {
            if tainted {
                Gate::Deny
            } else {
                Gate::Red
            }
        }
        Action::DispatchAgent => {
            if tainted {
                Gate::Red // the child inherits the taint
            } else {
                Gate::Allow
            }
        }
        // Never `Deny`: reading a thread and replying to it is the point of a connector. The control is the
        // operator's synchronous approval of the exact payload, at every taint level (specs/001 §5.2 note).
        Action::ConnectorWrite => Gate::Red,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn connector_writes_are_red_at_every_taint_level_and_never_denied() {
        for t in [Tier::L0, Tier::L1, Tier::L2, Tier::L3, Tier::L4, Tier::L5] {
            assert_eq!(gate(t, Action::ConnectorWrite), Gate::Red, "{}", t);
        }
        assert_eq!(gate(Tier::L0, Action::Outbound), Gate::Deny, "ordinary outbound stays denied when tainted");
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn the_ceiling_starts_at_l5_and_only_external_input_lowers_it() {
        let mut s = Span::root("sp1");
        assert_eq!(s.ceiling(), Tier::L5);
        for local in [Tier::L3, Tier::L4, Tier::L5] {
            assert!(!s.observe_input(local), "{} is local and must not lower the ceiling", local);
        }
        assert_eq!(s.ceiling(), Tier::L5);
        assert!(s.observe_input(Tier::L2));
        assert!(!s.observe_input(Tier::L3), "a local input after an external one changes nothing");
        assert_eq!(s.ceiling(), Tier::L2);
        assert!(s.observe_input(Tier::L0));
        assert!(!s.observe_input(Tier::L2), "it never goes back up");
        assert_eq!(s.ceiling(), Tier::L0);
    }

    #[test]
    fn a_child_inherits_the_parents_ceiling_and_cannot_launder_it() {
        let mut parent = Span::root("p");
        parent.observe_input(Tier::L0);
        let mut child = parent.child("c");
        assert_eq!(child.ceiling(), Tier::L0);
        child.observe_input(Tier::L5);
        assert_eq!(child.ceiling(), Tier::L0);
        assert_eq!(Span::root("fresh").child("c").ceiling(), Tier::L5);
    }

    #[test]
    fn what_a_span_writes_is_stamped_by_ceiling_and_verification() {
        for (ceiling, check, want) in [
            (Tier::L5, OutputCheck::Unverified, Tier::L3),
            (Tier::L5, OutputCheck::Verified, Tier::L4),
            (Tier::L2, OutputCheck::Unverified, Tier::L2),
            (Tier::L2, OutputCheck::Verified, Tier::L2),
            (Tier::L0, OutputCheck::Verified, Tier::L0),
            (Tier::L0, OutputCheck::Ratified, Tier::L5),
        ] {
            assert_eq!(output_tier(ceiling, check), want, "{} {:?}", ceiling, check);
        }
        let mut s = Span::root("s");
        s.observe_input(Tier::L1);
        assert_eq!(s.output_tier(OutputCheck::Verified), Tier::L1);
    }

    #[test]
    fn origin_defaults_follow_the_spec() {
        let r = OriginRules::with_defaults();
        assert_eq!(origin_default(Zone::Release, "agents/x/AGENT.md", &r, true), Tier::L5);
        assert_eq!(origin_default(Zone::Project, "src/lib.rs", &r, true), Tier::L4);
        assert_eq!(origin_default(Zone::Project, "src/lib.rs", &r, false), Tier::L3, "edited since the base commit");
        for p in ["vendor/x/y.js", "third_party/z.c", "node_modules/a/b.js"] {
            assert_eq!(origin_default(Zone::Project, p, &r, true), Tier::L0, "{}", p);
        }
        assert_eq!(origin_default(Zone::Team, "context/a.md", &r, true), Tier::L2);
        assert_eq!(origin_default(Zone::Local, "missions/m/notes.md", &r, true), Tier::L3);
    }

    #[test]
    fn the_index_overrides_origin_defaults_by_exact_content_hash_and_ignores_frontmatter() {
        let r = OriginRules::with_defaults();
        let mut idx = Index::new();
        idx.stamp(Zone::Team, "context/a.md", "hash1", Tier::L5);
        assert_eq!(idx.tier_of(Zone::Team, "context/a.md", "hash1", &r, true), Tier::L5);
        assert_eq!(idx.tier_of(Zone::Team, "context/a.md", "hash2", &r, true), Tier::L2, "changed content falls back to the origin default");
        assert_eq!(idx.tier_of(Zone::Team, "context/b.md", "hash1", &r, true), Tier::L2);
    }

    #[test]
    fn the_index_rebuilds_from_stamp_events() {
        let e = EventEnvelope::new("m", "provenance.stamp", Index::stamp_payload(Zone::Local, "missions/m/a.md", "h", Tier::L3));
        let other = EventEnvelope::new("m", "mission.note", json!({"zone": "local", "path": "x", "blake3": "y", "tier": "L5"}));
        let mut got = Index::new();
        got.apply_event(&e);
        got.apply_event(&other);
        assert_eq!(got.lookup(Zone::Local, "missions/m/a.md", "h"), Some(Tier::L3));
        assert_eq!(got.lookup(Zone::Local, "x", "y"), None, "only provenance.stamp events count");
    }

    #[test]
    fn the_provenance_gate_blocks_protected_writes_below_l4() {
        for p in ["context/heuristics.md", "REVIEW.md", "governance.md", "memory/agents/builder/lessons.md", "agents/builder/AGENT.md", "skills/custom/x/SKILL.md"] {
            assert!(gate_write(p, Tier::L3).is_err(), "{}", p);
            assert!(gate_write(p, Tier::L0).is_err(), "{}", p);
            assert!(gate_write(p, Tier::L4).is_ok(), "{}", p);
        }
        assert!(gate_write("missions/m/notes.md", Tier::L0).is_ok(), "ordinary artifacts are not gated");
        assert!(gate_write("REVIEW.md", Tier::L2).unwrap_err().to_string().contains("ratified proposals"));
    }

    #[test]
    fn gated_paths_are_recognised_under_worktree_and_project_prefixes() {
        for p in ["@worktree/skills/x/SKILL.md", "@worktree/agents/b/AGENT.md", "@project/app/skills/x/SKILL.md", "@Worktree/REVIEW.md", "./skills/x", "@project/app/context/heuristics.md"] {
            assert!(is_gated_path(p), "{}", p);
            assert!(gate_write(p, Tier::L1).is_err(), "{}", p);
        }
        for p in ["@worktree/src/skills.rs", "@project/skills", "@worktree/docs/agents.md"] {
            assert!(!is_gated_path(p), "{}", p);
        }
    }

    #[test]
    fn tainted_mission_output_defaults_to_l0_in_a_later_mission() {
        let r = OriginRules::with_defaults();
        for p in ["missions/a/research/web.md", "missions/a/research/deep/x.md", "missions/a/reports/brief.md"] {
            assert_eq!(origin_default(Zone::Local, p, &r, true), Tier::L0, "{}", p);
        }
        for p in ["missions/a/notes.md", "missions/a/specs/s.md", "memory/x.md"] {
            assert_eq!(origin_default(Zone::Local, p, &r, true), Tier::L3, "{}", p);
        }
        // Stamps are per mission: a fresh index (mission B) has no entry, so the default applies.
        let (mut a, b) = (Index::new(), Index::new());
        a.stamp(Zone::Local, "missions/a/research/web.md", "h", Tier::L0);
        assert_eq!(b.tier_of(Zone::Local, "missions/a/research/web.md", "h", &r, true), Tier::L0);
    }

    #[test]
    fn the_taint_gated_action_table() {
        for ceiling in [Tier::L5, Tier::L4, Tier::L3, Tier::L2] {
            assert_eq!(gate(ceiling, Action::Read), Gate::Allow);
            assert_eq!(gate(ceiling, Action::WorktreeWrite), Gate::Allow, "per HITL tier");
            assert_eq!(gate(ceiling, Action::Outbound), Gate::Red);
            assert_eq!(gate(ceiling, Action::DispatchAgent), Gate::Allow);
        }
        for ceiling in [Tier::L1, Tier::L0] {
            assert_eq!(gate(ceiling, Action::Read), Gate::Allow);
            assert_eq!(gate(ceiling, Action::WorktreeWrite), Gate::Deny);
            assert_eq!(gate(ceiling, Action::Outbound), Gate::Deny);
            assert_eq!(gate(ceiling, Action::DispatchAgent), Gate::Red);
        }
    }

    #[test]
    fn tier_names_and_parsing() {
        for t in [Tier::L0, Tier::L1, Tier::L2, Tier::L3, Tier::L4, Tier::L5] {
            assert_eq!(Tier::parse(t.as_str()), Some(t));
        }
        assert_eq!(Tier::L0.name(), "external_untrusted");
        assert_eq!(Tier::L5.name(), "user_ratified");
        assert_eq!(Tier::parse("L6"), None);
    }
}
