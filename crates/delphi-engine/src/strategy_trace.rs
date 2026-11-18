//! `strategy_trace`: does a strategy lead to action? (`specs/124` §6.2)
//!
//! A strategy document in the layout of `skills/strategy-kernel/SKILL.md` has three sections: a diagnosis (claims `D1…`, each with
//! a falsifier), guiding policies (`P1…`, each naming the diagnosis claims it answers) and actions (`A1…`, each naming the policies
//! it carries out). This module reads the links action → policy → diagnosis and reports where one is missing.
//!
//! It is a parser and a few set lookups: no model call, no write to the workspace. Like `check_alignment` (`align_run.rs`,
//! `specs/026` §5) it is offered as a tool, reads a path through the same guarded path as `read_file` (so scope, tier and taint
//! behave identically), and returns derived text. Its findings are advice (`P2`): they decide nothing and block nothing.
//!
//! It is offered to a role whose manifest authorizes one of the two skills that define the layout, so the tool goes where the
//! layout is taught and needs no `execute` name of its own.

use crate::engine::{tool_result_error, Engine, SpanCtx};
use crate::mission::EngineError;
use crate::tools::{self, ToolResult};
use delphi_adapter::claude::ToolDef;
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const TOOL: &str = "strategy_trace";
/// The skills that teach the layout. A role authorized for either is offered the tool.
pub(crate) const SKILLS: [&str; 2] = ["strategy-kernel", "strategy-cascade"];
/// The most text a call may pass inline. A document read by path is bounded by the pointer reader instead.
const MAX_TEXT_BYTES: usize = 64 * 1024;

pub(crate) fn def() -> ToolDef {
    ToolDef {
        name: TOOL.into(),
        description: "Check that a strategy document leads to action. It reads the fixed layout of the `strategy-kernel` skill (`## Diagnosis` with claims `D1…` and a `Falsifier:` each, `## Guiding policies` with `P1 (D1): …`, `## Actions` with `A1 (P1): …`) and reports an action that names no policy, a policy that names no diagnosis, a diagnosis with no falsifier, a policy with no action, a duplicate id and a reference to an id that does not exist. Anywhere in the document, a line labelled `Forecast` must carry a date (`YYYY-MM-DD`) and a `Tripwire:` (an observable that would show it wrong); one without either is reported. Pass `path` (a file you may read) or `text` (the document itself), not both. Deterministic and free. Findings are advice (P2): they never block anything, and a clean trace says only that the links exist, not that the strategy is good.".into(),
        input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}}),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// `TRACE-ACT-01` orphan action; `TRACE-POL-01` unfounded policy; `TRACE-POL-02` policy with no action; `TRACE-DIA-01` diagnosis with no
    /// falsifier; `TRACE-ID-01` duplicate id; `TRACE-REF-01` reference to an id the document does not define; `TRACE-DOC-01` a section of the
    /// layout is missing or empty; `TRACE-FOR-01` a line labelled `Forecast` with no date or no tripwire (`specs/124` §5).
    pub code: &'static str,
    /// Always `P2`: a trace finding advises, it never blocks (`specs/124` §6.2).
    pub level: &'static str,
    pub id: String,
    pub message: String,
}

impl Finding {
    fn new(code: &'static str, id: &str, message: String) -> Finding {
        Finding { code, level: "P2", id: id.to_string(), message }
    }

    pub fn line(&self) -> String {
        format!("{} ({}) {}: {}", self.code, self.level, self.id, self.message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Diagnosis,
    Policies,
    Actions,
}

impl Section {
    fn letter(self) -> char {
        match self {
            Section::Diagnosis => 'D',
            Section::Policies => 'P',
            Section::Actions => 'A',
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Section::Diagnosis => "## Diagnosis",
            Section::Policies => "## Guiding policies",
            Section::Actions => "## Actions",
        }
    }

    fn of_heading(title: &str) -> Option<Section> {
        match title.trim().trim_end_matches(':').to_ascii_lowercase().as_str() {
            "diagnosis" => Some(Section::Diagnosis),
            "guiding policies" | "guiding policy" => Some(Section::Policies),
            "actions" | "coherent actions" => Some(Section::Actions),
            _ => None,
        }
    }
}

/// One `D`, `P` or `A` item as written.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    id: String,
    /// The ids in the parentheses after the item's own id, in the order written.
    refs: Vec<String>,
    /// Named fields found on the item's line or the lines under it (`falsifier`, `owner`, `measure`; `tripwire` is read for a
    /// forecast and means nothing here), when they say something.
    fields: BTreeSet<&'static str>,
}

const FIELDS: [&str; 4] = ["falsifier", "owner", "measure", "tripwire"];
/// A field whose whole value is one of these says nothing, and counts as absent.
const PLACEHOLDERS: [&str; 8] = ["tbd", "todo", "none", "n/a", "na", "?", "-", "unknown"];

/// `D1`, `p12`: one letter of `DPA` and digits, returned in upper case.
fn as_id(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let first = chars.next()?.to_ascii_uppercase();
    let rest = chars.as_str();
    (matches!(first, 'D' | 'P' | 'A') && !rest.is_empty() && rest.len() <= 6 && rest.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{}{}", first, rest))
}

/// A line without its list marker, heading marks and emphasis, so `- **D1:** x` and `### D1: x` read alike.
fn bare(line: &str) -> String {
    let t = line.trim();
    let t = t.trim_start_matches('#').trim_start();
    let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")).unwrap_or(t);
    t.replace("**", "").replace("__", "").trim().to_string()
}

/// An item line: `D1: text`, `P1 (D1, D2): text`, `A3 (P1) text`. `None` when the line is not one for this section.
fn item_line(bare: &str, letter: char) -> Option<(String, Vec<String>, String)> {
    let end = bare.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(bare.len());
    let id = as_id(&bare[..end]).filter(|id| id.starts_with(letter))?;
    let rest = bare[end..].trim_start();
    let (refs, rest) = match rest.strip_prefix('(') {
        Some(inner) => {
            let close = inner.find(')')?;
            let refs = inner[..close].split(|c: char| !c.is_ascii_alphanumeric()).filter_map(as_id).collect();
            (refs, inner[close + 1..].trim_start())
        }
        None => (Vec::new(), rest),
    };
    // An id is followed by its references, a colon or the end of the line; `Diagnosis`, `P1s` and `A1 is` are prose.
    let text = match rest.strip_prefix(':') {
        Some(t) => t,
        None if rest.is_empty() || !refs.is_empty() => rest,
        None => return None,
    };
    Some((id, refs, text.trim().to_string()))
}

/// The named fields a line carries with a value that says something: at its start, or after the item's own text.
fn fields_in(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for f in FIELDS {
        let key = format!("{}:", f);
        let Some(at) = lower.find(&key) else { continue };
        if at > 0 && lower.as_bytes()[at - 1].is_ascii_alphanumeric() {
            continue;
        }
        let after = &lower[at + key.len()..];
        // The value ends where the next field on the same line begins (`owner: ops | measure: p99`).
        let end = FIELDS.iter().filter_map(|o| after.find(&format!("{}:", o))).min().unwrap_or(after.len());
        let value = after[..end].trim().trim_matches(|c: char| matches!(c, '|' | ';' | ',' | '.')).trim();
        if !value.is_empty() && !PLACEHOLDERS.contains(&value) {
            found.push(f);
        }
    }
    found
}

/// The items of each section, in the order written. Text inside a code fence is not read (an example is not the strategy).
fn parse(text: &str) -> (BTreeMap<Section, Vec<Item>>, BTreeSet<Section>) {
    let mut items: BTreeMap<Section, Vec<Item>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let (mut section, mut fenced) = (None, false);
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if t.starts_with('#') {
            let level = t.bytes().take_while(|b| *b == b'#').count();
            let title = t[level..].replace("**", "");
            if level <= 2 {
                // A first- or second-level heading opens a section of the layout or closes the one that was open.
                section = Section::of_heading(&title);
                if let Some(s) = section {
                    seen.insert(s);
                }
                continue;
            }
        }
        let Some(s) = section else { continue };
        let b = bare(line);
        if let Some((id, refs, rest)) = item_line(&b, s.letter()) {
            items.entry(s).or_default().push(Item { id, refs, fields: fields_in(&rest).into_iter().collect() });
        } else if let Some(last) = items.get_mut(&s).and_then(|v| v.last_mut()) {
            last.fields.extend(fields_in(&b));
        }
    }
    (items, seen)
}

/// The findings of a strategy document, and notes that are not findings (an action with no owner or measure).
pub fn trace(text: &str) -> (Vec<Finding>, Vec<String>) {
    let (items, seen) = parse(text);
    let of = |s: Section| items.get(&s).map(|v| v.as_slice()).unwrap_or(&[]);
    let ids = |s: Section| of(s).iter().map(|i| i.id.as_str()).collect::<BTreeSet<_>>();
    let (mut findings, mut notes) = (Vec::new(), Vec::new());

    for s in [Section::Diagnosis, Section::Policies, Section::Actions] {
        if !seen.contains(&s) {
            findings.push(Finding::new("TRACE-DOC-01", s.heading(), "the section is missing, so nothing under it could be traced".into()));
        } else if of(s).is_empty() {
            findings.push(Finding::new("TRACE-DOC-01", s.heading(), format!("the section has no item written as `{}1: …`", s.letter())));
        }
        let mut once = BTreeSet::new();
        let mut twice = BTreeSet::new();
        for i in of(s) {
            if !once.insert(i.id.as_str()) && twice.insert(i.id.as_str()) {
                findings.push(Finding::new("TRACE-ID-01", &i.id, "the id is used more than once, so a reference to it is ambiguous".into()));
            }
        }
    }

    for d in of(Section::Diagnosis) {
        if !d.fields.contains("falsifier") {
            findings.push(Finding::new("TRACE-DIA-01", &d.id, "no falsifier: say what observation would show this diagnosis wrong".into()));
        }
    }

    let diagnoses = ids(Section::Diagnosis);
    let policies = ids(Section::Policies);
    // A link holds only when it names an id the document defines; an unknown id is reported and does not count.
    let mut links = |item: &Item, known: &BTreeSet<&str>, code: &'static str, none: &str| {
        let unknown: Vec<&str> = item.refs.iter().map(|r| r.as_str()).filter(|r| !known.contains(r)).collect();
        for r in &unknown {
            findings.push(Finding::new("TRACE-REF-01", &item.id, format!("names {}, which the document does not define", r)));
        }
        if unknown.len() == item.refs.len() {
            findings.push(Finding::new(code, &item.id, none.to_string()));
        }
    };
    for p in of(Section::Policies) {
        links(p, &diagnoses, "TRACE-POL-01", "unfounded policy: it names no diagnosis claim it answers (write `P1 (D1): …`)");
    }
    let mut carried = BTreeSet::new();
    for a in of(Section::Actions) {
        links(a, &policies, "TRACE-ACT-01", "orphan action: it names no guiding policy it carries out (write `A1 (P1): …`)");
        carried.extend(a.refs.iter().map(|r| r.as_str()));
        let missing: Vec<&str> = ["owner", "measure"].into_iter().filter(|f| !a.fields.contains(f)).collect();
        if !missing.is_empty() {
            notes.push(format!("{} has no {}", a.id, missing.join(" and no ")));
        }
    }
    let mut reported = BTreeSet::new();
    for p in of(Section::Policies) {
        if !carried.contains(p.id.as_str()) && reported.insert(p.id.as_str()) {
            findings.push(Finding::new("TRACE-POL-02", &p.id, "no action carries this policy out".into()));
        }
    }
    findings.extend(forecasts(text));
    (findings, notes)
}

/// What follows the label of a line labelled as a forecast: `Forecast: …`, `- **Forecast** (2027-03-31): …`, `` `forecast`: … ``.
/// `Forecasts are hard` and a heading `## Forecast` are prose, not a label.
fn forecast_label(bare: &str) -> Option<&str> {
    let rest = bare.get(..8).filter(|l| l.eq_ignore_ascii_case("forecast")).map(|_| bare[8..].trim_start())?;
    (rest.starts_with(':') || rest.starts_with('(')).then_some(rest)
}

/// Does the text name a day, `YYYY-MM-DD`? What follows `tripwire:` is not read: the day a tripwire is looked at is not the day
/// the forecast is for.
fn has_date(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let own = lower.find("tripwire:").map_or(lower.as_str(), |at| &lower[..at]);
    own.split(|c: char| !(c.is_ascii_digit() || c == '-')).any(|w| w.len() == 10 && crate::frontmatter::entry_date_ok(w))
}

/// `TRACE-FOR-01` (`specs/124` §5 item 1): every line labelled `Forecast`, in any section and outside a code fence, with the lines
/// written under it (indented deeper, or a `Tripwire:` line), must carry a date and a tripwire that says something. Only a labelled
/// line is seen: a forecast written as a plain sentence is not found.
fn forecasts(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    // The forecast being read: its line number, its indent, and whether a date and a tripwire were found so far.
    let mut open: Option<(usize, usize, bool, bool)> = None;
    let mut close = |open: &mut Option<(usize, usize, bool, bool)>| {
        if let Some((line, _, dated, tripwire)) = open.take() {
            let missing: Vec<&str> = [(dated, "no date (write `Forecast (YYYY-MM-DD): …`)"), (tripwire, "no tripwire (write `Tripwire: …`, an observable that would show it wrong)")].into_iter().filter(|(has, _)| !has).map(|(_, what)| what).collect();
            if !missing.is_empty() {
                out.push(Finding::new("TRACE-FOR-01", &format!("forecast, line {}", line), format!("a forecast with {}", missing.join(" and "))));
            }
        }
    };
    let mut fenced = false;
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            close(&mut open);
            continue;
        }
        if fenced {
            continue;
        }
        let indent = line.len() - t.len();
        let b = bare(line).replace('`', "");
        if let Some(rest) = (!t.starts_with('#')).then(|| forecast_label(&b)).flatten() {
            close(&mut open);
            open = Some((n + 1, indent, has_date(rest), fields_in(rest).contains(&"tripwire")));
        } else if let Some((_, at, dated, tripwire)) = open.as_mut() {
            let is_tripwire = b.get(..9).is_some_and(|l| l.eq_ignore_ascii_case("tripwire:"));
            if t.is_empty() || t.starts_with('#') || !(indent > *at || is_tripwire) {
                close(&mut open);
            } else {
                *dated |= has_date(&b);
                *tripwire |= fields_in(&b).contains(&"tripwire");
            }
        }
    }
    close(&mut open);
    out
}

/// What the tool answers: the findings, the notes, and what a clean result does and does not mean.
pub fn report(what: &str, text: &str) -> (String, Vec<Finding>) {
    let (findings, notes) = trace(text);
    let mut out = if findings.is_empty() {
        format!("{}: the trace is complete: every action names a policy, every policy names a diagnosis and has an action, and every diagnosis has a falsifier. This says the links exist, not that the strategy is good.\n", what)
    } else {
        format!("{}: {} finding(s), all P2 (advice, nothing is blocked):\n{}\n", what, findings.len(), findings.iter().map(|f| f.line()).collect::<Vec<_>>().join("\n"))
    };
    for n in notes {
        out.push_str(&format!("note: {}\n", n));
    }
    (out, findings)
}

impl Engine {
    /// Does this span get the tool: a role authorized for one of the layout's skills, with tools.
    pub(crate) fn offers_strategy_trace(&self, sc: &SpanCtx) -> bool {
        !sc.toolless && authorized(&sc.manifest.skills)
    }

    pub(crate) async fn exec_strategy_trace(&self, sc: &mut SpanCtx, args: &Value) -> Result<ToolResult, EngineError> {
        if !authorized(&sc.manifest.skills) {
            return Ok(tool_result_error("this role's manifest does not authorize the strategy skills"));
        }
        let path = args["path"].as_str().filter(|p| !p.is_empty());
        let text = args["text"].as_str().filter(|t| !t.trim().is_empty());
        let mut events: Vec<EventEnvelope> = Vec::new();
        let (what, document, cut) = match (path, text) {
            (Some(_), Some(_)) | (None, None) => return Ok(tool_result_error("strategy_trace needs a `path` or a `text`, not both")),
            (None, Some(t)) if t.len() > MAX_TEXT_BYTES => return Ok(tool_result_error(format!("`text` is over {} bytes; write the document to a file and pass its `path`", MAX_TEXT_BYTES))),
            (None, Some(t)) => ("the text".to_string(), t.to_string(), false),
            (Some(p), None) if p.len() > 1024 || p.contains("#L") => return Ok(tool_result_error("`path` is a file path, without a line range")),
            (Some(p), None) => {
                let before = sc.span.ceiling();
                let now = self.now();
                let (read, stamps) = {
                    let mut idx = sc.m.index.lock().unwrap();
                    let mut ctx = tools::ToolCtx { guard: &sc.guard, span: &mut sc.span, index: &mut idx, rules: &self.rules, probe: &*self.probe, role: &sc.role, mission_id: &sc.m.id, now_rfc3339: &now, stamps: vec![], events: vec![] };
                    let r = tools::read_pointer(&mut ctx, p);
                    (r, std::mem::take(&mut ctx.stamps))
                };
                events.extend(stamps.iter().map(|s| sc.ev("provenance.stamp", delphi_core::provenance::Index::stamp_payload(s.zone, &s.path, &s.blake3, s.tier))));
                if sc.span.ceiling() != before {
                    events.push(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": TOOL})));
                }
                match read {
                    Err(e) => {
                        sc.m.append_batch(events).await?;
                        return Ok(tool_result_error(e));
                    }
                    Ok((t, _tier)) => {
                        let cut = t.contains("\n[excerpt cut:");
                        (p.to_string(), t, cut)
                    }
                }
            }
        };
        let (mut out, findings) = report(&what, &document);
        if cut {
            out.push_str("note: the file is longer than one read, so only its first part was traced; a finding about a missing link may be answered further down. A strategy in this layout fits on one page.\n");
        }
        events.push(sc.ev("strategy.trace", json!({"path": path, "cut": cut, "findings": findings.iter().map(|f| json!({"code": f.code, "level": f.level, "id": f.id})).collect::<Vec<_>>()})));
        sc.m.append_batch(events).await?;
        Ok(ToolResult::ok(out))
    }
}

fn authorized(skills: &[String]) -> bool {
    skills.iter().any(|s| SKILLS.contains(&s.as_str()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const CLEAN: &str = "# Strategy: faster checkout\n\n## Diagnosis\n- D1: Checkout is slow because every page load calls the tax service.\n  Falsifier: p99 stays above 2 s with the tax call cached.\n- D2: Two teams own the checkout path and neither owns its latency.\n  Falsifier: an incident review names one owner.\n\n## Guiding policies\n- P1 (D1): Take the tax call off the page-load path.\n- P2 (D2): One team owns checkout latency.\n\n## Actions\n- A1 (P1): Cache tax rates per region.\n  Owner: payments\n  Measure: p99 under 1 s by 2027-01-31\n  Stop doing: per-request tax lookups\n- A2 (P2, P1): Move the latency budget to the payments team. | owner: payments lead | measure: budget in the team charter\n";

    fn codes(text: &str) -> Vec<String> {
        trace(text).0.iter().map(|f| format!("{} {}", f.code, f.id)).collect()
    }

    // specs/124 AC-4: a clean trace passes. The function takes text and returns text: there is no model to call.
    #[test]
    fn a_clean_trace_has_no_finding_and_no_note() {
        let (findings, notes) = trace(CLEAN);
        assert!(findings.is_empty(), "{:?}", findings);
        assert!(notes.is_empty(), "{:?}", notes);
        let (out, _) = report("specs/s.md", CLEAN);
        assert!(out.starts_with("specs/s.md: the trace is complete") && out.contains("not that the strategy is good"), "{}", out);
    }

    // specs/124 AC-4: a planted orphan action, an unfounded policy and a diagnosis with no falsifier each fail.
    #[test]
    fn a_planted_orphan_action_an_unfounded_policy_and_a_diagnosis_with_no_falsifier_are_each_found() {
        let orphan = CLEAN.replace("- A1 (P1): Cache", "- A1: Cache");
        assert_eq!(codes(&orphan), ["TRACE-ACT-01 A1"]);
        let unfounded = CLEAN.replace("- P2 (D2): One", "- P2: One");
        assert_eq!(codes(&unfounded), ["TRACE-POL-01 P2"]);
        let unfalsifiable = CLEAN.replace("  Falsifier: an incident review names one owner.\n", "");
        assert_eq!(codes(&unfalsifiable), ["TRACE-DIA-01 D2"]);
        let all = orphan.replace("- P2 (D2): One", "- P2: One").replace("  Falsifier: an incident review names one owner.\n", "");
        assert_eq!(codes(&all), ["TRACE-DIA-01 D2", "TRACE-POL-01 P2", "TRACE-ACT-01 A1"]);
        let (out, findings) = report("the text", &all);
        assert!(out.starts_with("the text: 3 finding(s), all P2 (advice, nothing is blocked):"), "{}", out);
        assert!(findings.iter().all(|f| f.level == "P2"));
    }

    #[test]
    fn a_policy_no_action_carries_out_is_found() {
        let text = CLEAN.replace("- A2 (P2, P1): Move", "- A2 (P1): Move");
        assert_eq!(codes(&text), ["TRACE-POL-02 P2"]);
    }

    #[test]
    fn a_duplicate_id_is_found_once_and_a_reference_to_an_unknown_id_does_not_count_as_a_link() {
        let dup = CLEAN.replace("- P2 (D2): One", "- P1 (D2): One").replace("(P2, P1)", "(P1)");
        assert_eq!(codes(&dup), ["TRACE-ID-01 P1"]);
        // A1 names only P9, which is not defined: the reference is reported and the action is an orphan. P1 is still carried by A2.
        let unknown = CLEAN.replace("- A1 (P1): Cache", "- A1 (P9): Cache");
        assert_eq!(codes(&unknown), ["TRACE-REF-01 A1", "TRACE-ACT-01 A1"]);
        // one known and one unknown: the link holds, the unknown id is still reported
        let mixed = CLEAN.replace("- A1 (P1): Cache", "- A1 (P1, P9): Cache");
        assert_eq!(codes(&mixed), ["TRACE-REF-01 A1"]);
        // a policy that names an action id, not a diagnosis id, is unfounded
        let wrong_kind = CLEAN.replace("- P1 (D1): Take", "- P1 (A1): Take");
        assert_eq!(codes(&wrong_kind), ["TRACE-REF-01 P1", "TRACE-POL-01 P1"]);
    }

    #[test]
    fn a_falsifier_that_says_nothing_is_no_falsifier() {
        for empty in ["Falsifier:", "Falsifier: TBD", "Falsifier: n/a", "**Falsifier:** none."] {
            let text = CLEAN.replace("Falsifier: an incident review names one owner.", empty);
            assert_eq!(codes(&text), ["TRACE-DIA-01 D2"], "{}", empty);
        }
        // on the item's own line, in bold, or as a nested bullet, a real one counts
        for form in ["- D2: Two teams own it. Falsifier: one owner is named.\n", "- **D2:** Two teams own it.\n  - **Falsifier:** one owner is named.\n", "### D2: Two teams own it.\n\nFalsifier: one owner is named.\n"] {
            let text = CLEAN.replace("- D2: Two teams own the checkout path and neither owns its latency.\n  Falsifier: an incident review names one owner.\n", form);
            assert!(codes(&text).is_empty(), "{} gave {:?}", form, codes(&text));
        }
    }

    #[test]
    fn a_missing_or_empty_section_is_a_finding_so_an_empty_document_does_not_pass() {
        assert_eq!(codes(""), ["TRACE-DOC-01 ## Diagnosis", "TRACE-DOC-01 ## Guiding policies", "TRACE-DOC-01 ## Actions"]);
        let no_actions = CLEAN.split("## Actions").next().unwrap().to_string();
        assert_eq!(codes(&no_actions), ["TRACE-DOC-01 ## Actions", "TRACE-POL-02 P1", "TRACE-POL-02 P2"]);
        let empty = format!("{}## Actions\nWe will act decisively.\n", no_actions);
        assert_eq!(codes(&empty), ["TRACE-DOC-01 ## Actions", "TRACE-POL-02 P1", "TRACE-POL-02 P2"]);
    }

    #[test]
    fn an_action_with_no_owner_or_measure_is_a_note_not_a_finding() {
        let text = CLEAN.replace("  Owner: payments\n", "").replace(" | measure: budget in the team charter", " | measure: tbd");
        let (findings, notes) = trace(&text);
        assert!(findings.is_empty(), "{:?}", findings);
        assert_eq!(notes, ["A1 has no owner", "A2 has no measure"]);
        assert!(report("x", &text).0.contains("note: A1 has no owner\n"));
    }

    #[test]
    fn prose_examples_in_a_fence_and_other_sections_are_not_read_as_items() {
        let text = format!("{}\n## Notes\n- A9: an idea that is not an action yet\n- P7: nor is this a policy\n\n```\n## Actions\n- A8: an example\n```\n", CLEAN.replace("## Actions\n", "## Actions\nA plan is what follows. P1s matter. A1 is first.\n"));
        assert!(codes(&text).is_empty(), "{:?}", codes(&text));
        // headings may differ in case and wording within the layout
        let relabelled = CLEAN.replace("## Guiding policies", "## Guiding Policy").replace("## Actions", "## Coherent actions:");
        assert!(codes(&relabelled).is_empty(), "{:?}", codes(&relabelled));
    }

    // The two skills teach the layout by example. Their examples are read here, so the text an agent learns from and the parser
    // cannot drift apart. Both skills must also say where their content came from (`specs/124` OD-3).
    #[test]
    fn the_layout_examples_in_the_two_skills_trace_clean_and_the_skills_say_their_sources_are_secondary() {
        for name in SKILLS {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills").join(name).join("SKILL.md");
            let skill = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
            let example = skill.split("```markdown\n").nth(1).and_then(|rest| rest.split("\n```").next()).unwrap_or_else(|| panic!("{} has no layout example", name));
            let (findings, notes) = trace(example);
            assert!(findings.is_empty() && notes.is_empty(), "{}: {:?} {:?}", name, findings, notes);
            assert!(skill.contains("`strategy_trace`") && skill.contains("\n## Done when\n") && skill.contains("\n## Pitfalls\n"), "{}", name);
            assert!(skill.contains("written from secondary summaries, not from the primary work") && skill.contains("OD-3"), "{}", name);
            assert!(skill.contains("authorized_agents:\n  - strategist\n"), "{}", name);
        }
        let strategist = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agents/strategist/AGENT.md")).unwrap();
        let manifest = crate::workspace::AgentManifest::parse(&strategist).unwrap();
        assert!(SKILLS.iter().all(|s| manifest.skills.iter().any(|m| m == s)), "the strategist is authorized for both, which is what offers it the tool");
    }

    // specs/124 AC-3: a forecast without a date or a tripwire is flagged; one with both is not.
    #[test]
    fn a_line_labelled_forecast_with_no_date_or_no_tripwire_is_found_and_one_with_both_is_not() {
        let good = "- Forecast (2027-03-31): checkout is the fastest step in both regions.\n  Tripwire: p99 above 1.5 s at the 2027-02-15 review.\n";
        assert!(codes(&format!("{}\n## Outlook\n{}", CLEAN, good)).is_empty());
        // in a traced section, between items, it is still read and takes nothing from the item above it
        assert!(codes(&CLEAN.replace("## Guiding policies\n", &format!("{}\n## Guiding policies\n", good))).is_empty());
        let with = |forecast: &str| trace(&format!("{}\n## Outlook\nSome prose.\n{}\nMore prose with a date, 2027-01-01, and a Tripwire: that belongs to nothing.\n", CLEAN, forecast)).0;
        let line = CLEAN.lines().count() + 4;
        for (forecast, missing) in [
            ("- Forecast: the tax service stays the slowest call.\n  Tripwire: another call is slower in the weekly review.", "no date"),
            ("- Forecast (next quarter): the tax service stays the slowest call.\n  Tripwire: seen in the 2027-02-15 review.", "no date"),
            ("- Forecast (2027-13-45): never.\n  Tripwire: something seen.", "no date"),
            ("- **Forecast** (2027-03-31): the tax service stays the slowest call.", "no tripwire"),
            ("Forecast (2027-03-31): the tax service stays the slowest call.\n  Tripwire: TBD", "no tripwire"),
            ("`forecast`: the tax service stays the slowest call.", "no date (write `Forecast (YYYY-MM-DD): …`) and no tripwire"),
        ] {
            let found = with(forecast);
            assert_eq!(found.len(), 1, "{}: {:?}", forecast, found);
            assert_eq!((found[0].code, found[0].level, found[0].id.as_str()), ("TRACE-FOR-01", "P2", format!("forecast, line {}", line).as_str()), "{}", forecast);
            assert!(found[0].message.starts_with("a forecast with ") && found[0].message.contains(missing), "{}: {}", forecast, found[0].message);
        }
        // other accepted forms: all on one line, a nested bullet, a paragraph with the tripwire on the next line
        for ok in ["Forecast (2027-03-31): it holds. Tripwire: p99 above 1.5 s.", "- forecast: by 2027-03-31 it holds.\n  - **Tripwire:** p99 above 1.5 s.", "Forecast (2027-03-31): it holds.\nTripwire: p99 above 1.5 s."] {
            assert!(with(ok).is_empty(), "{}: {:?}", ok, with(ok));
        }
        // prose about forecasts, a heading and a fenced example are not forecasts; an unlabelled forecast is not seen at all
        for prose in ["Forecasts are hard.", "### Forecast", "```\n- Forecast: an example\n```", "Sign-ups will double by 2027-03-31."] {
            assert!(with(prose).is_empty(), "{}", prose);
        }
        // two forecasts, one bad: only the bad one, by its own line
        let two = with("- Forecast (2027-03-31): a.\n  Tripwire: b.\n- Forecast (2027-06-30): c.");
        assert_eq!(two.iter().map(|f| f.id.clone()).collect::<Vec<_>>(), [format!("forecast, line {}", line + 2)]);
        let (out, _) = report("x", &format!("{}\n- Forecast: soon.\n", CLEAN));
        assert!(out.starts_with("x: 1 finding(s), all P2 (advice, nothing is blocked):\nTRACE-FOR-01 (P2) forecast, line "), "{}", out);
    }

    // The forecast example in `strategy-kernel` is the layout an agent learns; it is read here so it cannot drift from the check.
    // The four skills that make forecasts and the two charters carry the rule (`specs/124` §5).
    #[test]
    fn the_forecast_example_in_the_skill_passes_and_the_forecasting_skills_carry_the_rule() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{}: {}", rel, e));
        let kernel = read("skills/strategy-kernel/SKILL.md");
        let example = kernel.split("```markdown\n").nth(2).and_then(|rest| rest.split("\n```").next()).expect("strategy-kernel has a forecast example");
        assert!(forecast_label(&bare(example.lines().next().unwrap())).is_some(), "{}", example);
        assert!(forecasts(example).is_empty(), "{:?}", forecasts(example));
        assert_eq!(forecasts(&example.replace("Tripwire:", "Note:")).len(), 1, "without its tripwire the same example is flagged");
        assert!(kernel.contains("`TRACE-FOR-01`"));
        for skill in ["strategy-kernel", "strategy-cascade", "roadmap-sequencing", "risk-roi-analysis"] {
            let text = read(&format!("skills/{}/SKILL.md", skill));
            assert!(text.contains("Forecast (") && text.contains("Tripwire:") && text.contains("an observable that would show it wrong"), "{}", skill);
        }
        assert!(read("agents/strategist/AGENT.md").contains("carries the label `forecast`, a date (`YYYY-MM-DD`) and a tripwire"));
    }

    #[test]
    fn the_same_text_always_gives_the_same_report() {
        let text = CLEAN.replace("- A1 (P1): Cache", "- A1: Cache");
        assert_eq!(report("x", &text), report("x", &text));
    }
}
