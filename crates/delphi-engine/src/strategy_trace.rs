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
        description: "Check that a strategy document leads to action. It reads the fixed layout of the `strategy-kernel` skill (`## Diagnosis` with claims `D1…` and a `Falsifier:` each, `## Guiding policies` with `P1 (D1): …`, `## Actions` with `A1 (P1): …`; a heading may be numbered, an item may follow a list marker, a number or a task box) and reports an action that names no policy, a policy that names no diagnosis, a diagnosis with no falsifier, a policy with no action, a duplicate id, a reference to an id that does not exist and an item line it could not read. A `text` wrapped whole in one code fence is read inside it. Anywhere in the document, a line labelled `Forecast` must carry a date (`YYYY-MM-DD`) and a `Tripwire:` (an observable that would show it wrong); one without either is reported. Pass `path` (a file you may read) or `text` (the document itself), not both. Deterministic and free. Findings are advice (P2): they never block anything, and a clean trace says only that the links exist, not that the strategy is good.".into(),
        input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}, "text": {"type": "string"}}}),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// `TRACE-ACT-01` orphan action; `TRACE-POL-01` unfounded policy; `TRACE-POL-02` policy with no action; `TRACE-DIA-01` diagnosis with no
    /// falsifier; `TRACE-ID-01` duplicate id; `TRACE-REF-01` reference to an id the document does not define; `TRACE-DOC-01` a section of the
    /// layout is missing or empty; `TRACE-DOC-02` a line that starts with an id of its section but could not be read as an item; `TRACE-FOR-01` a line labelled `Forecast` with no date or no tripwire (`specs/124` §5).
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

    /// The section a heading opens. The title may carry a number (`2. Guiding policies`, `3) Actions`) and more words after the
    /// section's own (`Actions and owners`, `Diagnosis: why checkout is slow`).
    fn of_heading(title: &str) -> Option<Section> {
        let lower = title.trim().to_ascii_lowercase();
        let digits = lower.bytes().take_while(u8::is_ascii_digit).count();
        let t = if digits > 0 { lower[digits..].trim_start_matches(['.', ')']).trim_start() } else { lower.as_str() };
        let starts = |w: &str| t.strip_prefix(w).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()));
        if starts("diagnosis") || starts("diagnoses") {
            Some(Section::Diagnosis)
        } else if starts("guiding policies") || starts("guiding policy") {
            Some(Section::Policies)
        } else if starts("actions") || starts("coherent actions") {
            Some(Section::Actions)
        } else {
            None
        }
    }

    /// How an item of the section is written, for a finding that says how to write one.
    fn example(self) -> &'static str {
        match self {
            Section::Diagnosis => "D1: …",
            Section::Policies => "P1 (D1): …",
            Section::Actions => "A1 (P1): …",
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
const PLACEHOLDERS: [&str; 3] = ["none", "na", "unknown"];
/// A field whose value starts with one of these says nothing yet (`TBD later`, `—`), and counts as absent.
const PENDING: [&str; 9] = ["tbd", "tbc", "todo", "n/a", "?", "-", "–", "—", "…"];

/// Does a field's value (lower case, trimmed) say something?
fn says_something(value: &str) -> bool {
    let first = value.split_whitespace().next().unwrap_or("").trim_end_matches(['.', ',', ';', ':', '!']);
    !(first.is_empty() || PLACEHOLDERS.contains(&value) || PENDING.contains(&first) || first.chars().all(|c| c == '?'))
}

/// `D1`, `p12`, `P01`: one letter of `DPA` and digits, returned in upper case without leading zeros (`P01` is `P1`).
fn as_id(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let first = chars.next()?.to_ascii_uppercase();
    let rest = chars.as_str();
    if !(matches!(first, 'D' | 'P' | 'A') && !rest.is_empty() && rest.len() <= 6 && rest.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let digits = rest.trim_start_matches('0');
    Some(format!("{}{}", first, if digits.is_empty() { "0" } else { digits }))
}

/// What follows a list marker (`- `, `* `, `+ `, `1. `, `1) `) and a task box (`[ ] `, `[x] `); `None` for a line that is not a
/// list item. `t` is the line without its indent.
fn after_marker(t: &str) -> Option<&str> {
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")).or_else(|| {
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        (1..=9).contains(&digits).then(|| &t[digits..]).and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")))
    })?;
    let rest = rest.trim_start();
    Some(["[ ] ", "[x] ", "[X] "].iter().find_map(|b| rest.strip_prefix(b)).unwrap_or(rest))
}

/// A line without its list marker, task box, heading marks and emphasis, so `- **D1:** x`, `1. D1: x`, `- [ ] *D1*: x` and
/// `### D1: x` read alike.
fn bare(line: &str) -> String {
    let t = line.trim();
    let t = t.trim_start_matches('#').trim_start();
    let t = after_marker(t).unwrap_or(t);
    let mut s = t.replace("**", "").replace("__", "").trim().to_string();
    // A single `*` or `_` around the first words: `*D1*: x`, `_Falsifier_: y`.
    if let Some(c) = s.chars().next().filter(|c| matches!(c, '*' | '_')) {
        if let Some(close) = s[1..].find(c) {
            s = format!("{}{}", &s[1..1 + close], &s[2 + close..]);
        }
    }
    s
}

/// The ids named in an item's parentheses. A range of one letter, `P1–P3` or `P1 - P3`, names every id in it (at most
/// `MAX_RANGE`); a reversed or longer one names its two ends.
fn refs_of(inner: &str) -> Vec<String> {
    let dashed = inner.replace(['–', '—'], "-").replace(" -", "-").replace("- ", "-");
    let mut out = Vec::new();
    for token in dashed.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        let ends: Vec<String> = token.split('-').filter_map(as_id).collect();
        if let [from, to] = ends.as_slice() {
            if let (Ok(a), Ok(b)) = (from[1..].parse::<u32>(), to[1..].parse::<u32>()) {
                if from[..1] == to[..1] && a < b && b - a < MAX_RANGE {
                    out.extend((a..=b).map(|n| format!("{}{}", &from[..1], n)));
                    continue;
                }
            }
        }
        out.extend(ends);
    }
    out
}

/// The most ids a range in an item's references may stand for.
const MAX_RANGE: u32 = 20;

/// An item's references and its text, as read from its line.
type Read = (Vec<String>, String);

/// The references and text after an item's own id: `(D1, D2): text`, `: (D1) text`, `- text`, `: text`. `None` when what follows
/// the id is not one of these (`P1s`, `A1 is`) or a parenthesis is not closed on the line.
fn item_rest(rest: &str) -> Option<Read> {
    let rest = rest.trim_start();
    let (refs, rest) = match rest.strip_prefix('(') {
        Some(inner) => {
            let close = inner.find(')')?;
            (refs_of(&inner[..close]), inner[close + 1..].trim_start())
        }
        None => (Vec::new(), rest),
    };
    // An id is followed by its references, a colon, a dash or the end of the line.
    let text = if let Some(t) = rest.strip_prefix(':') {
        t
    } else if let Some(t) = rest.strip_prefix(['-', '–', '—']).filter(|t| t.is_empty() || t.starts_with(char::is_whitespace)) {
        t
    } else if rest.is_empty() || !refs.is_empty() {
        rest
    } else {
        return None;
    };
    let text = text.trim();
    // References written after the colon: `P1: (D1) text`.
    if refs.is_empty() {
        if let Some((inner, after)) = text.strip_prefix('(').and_then(|t| t.split_once(')')) {
            return Some((refs_of(inner), after.trim().to_string()));
        }
    }
    Some((refs, text.to_string()))
}

/// What an item line of this section says: `None` when the line does not start with an id of the section, `Some((id, None))` when
/// it does but cannot be read as an item. A line counts as unreadable only when it is marked as an item (a list item or a heading)
/// or its id is followed by a parenthesis; a sentence that starts with an id (`D1 is the main one`) is prose.
fn item_line(bare: &str, letter: char, marked: bool) -> Option<(String, Option<Read>)> {
    let end = bare.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(bare.len());
    let id = as_id(&bare[..end]).filter(|id| id.starts_with(letter))?;
    match item_rest(&bare[end..]) {
        Some(read) => Some((id, Some(read))),
        None if marked || bare[end..].trim_start().starts_with('(') => Some((id, None)),
        None => None,
    }
}

/// Does the line begin with a named field (`Falsifier: …`)?
fn starts_with_field(bare: &str) -> bool {
    let lower = bare.to_ascii_lowercase();
    FIELDS.iter().any(|f| lower.strip_prefix(f).is_some_and(|r| r.starts_with(':')))
}

/// The named fields a line carries with a value that says something: at its start, or after the item's own text.
fn fields_in(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for f in FIELDS {
        let key = format!("{}:", f);
        // The first place the key stands as a word of its own: `unfalsifier:` is not the field.
        let Some(at) = lower.match_indices(&key).map(|(at, _)| at).find(|at| *at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric()) else { continue };
        let after = &lower[at + key.len()..];
        // The value ends where the next field on the same line begins (`owner: ops | measure: p99`).
        let end = FIELDS.iter().filter_map(|o| after.find(&format!("{}:", o))).min().unwrap_or(after.len());
        let value = after[..end].trim().trim_matches(|c: char| matches!(c, '|' | ';' | ',' | '.')).trim();
        if says_something(value) {
            found.push(f);
        }
    }
    found
}

/// The code fence a line is in. A fence opens on a line that starts with three or more backticks or tildes (a backtick fence's
/// info string has no backtick, so ```` ```toml``` is … ```` is inline code) and closes only on a line of the same mark, at least as
/// long, and nothing else.
#[derive(Default)]
struct Fence(Option<(char, usize)>);

impl Fence {
    /// Is the line part of a fence: its opening, its inside or its closing?
    fn skip(&mut self, line: &str) -> bool {
        let t = line.trim();
        let run = |c: char| t.chars().take_while(|x| *x == c).count();
        match self.0 {
            Some((c, n)) => {
                if run(c) >= n && t.chars().all(|x| x == c) {
                    self.0 = None;
                }
                true
            }
            None => {
                for c in ['`', '~'] {
                    let n = run(c);
                    if n >= 3 && !(c == '`' && t[n..].contains('`')) {
                        self.0 = Some((c, n));
                        return true;
                    }
                }
                false
            }
        }
    }
}

/// What a document holds: the items of each section in the order written, the sections it has, and the lines that start with an
/// id of their section but could not be read (section, id, line).
struct Parsed {
    items: BTreeMap<Section, Vec<Item>>,
    seen: BTreeSet<Section>,
    unread: Vec<(Section, String, String)>,
}

/// The item that the lines under it give fields to: its indent, whether it was written as a heading (its body runs to the next
/// heading or item, across blank lines), and whether a blank line has followed it.
struct Open {
    indent: usize,
    heading: bool,
    blank: bool,
}

/// The items of each section, in the order written. Text inside a code fence is not read (an example is not the strategy).
///
/// A heading of level 1 or 2 opens a section of the layout or closes the one that was open; a level-3 heading does the same when
/// no level-1 or level-2 section of the layout is open (`## Strategy` then `### Diagnosis`). A field counts for an item when it is
/// on the item's line or on a line directly under it: indented deeper, or starting with the field (`Falsifier: …`). A list item
/// ends at a blank line not followed by an indented one, at prose, at another list item, a heading or a line it could not read.
fn parse(text: &str) -> Parsed {
    let mut p = Parsed { items: BTreeMap::new(), seen: BTreeSet::new(), unread: Vec::new() };
    // The open section and the level of its heading.
    let mut section: Option<(Section, usize)> = None;
    let mut open: Option<Open> = None;
    let mut fence = Fence::default();
    for line in text.lines() {
        if fence.skip(line) {
            open = None;
            continue;
        }
        let t = line.trim_start();
        let indent = line.len() - t.len();
        let heading = t.starts_with('#');
        if heading {
            open = None;
            let level = t.bytes().take_while(|b| *b == b'#').count();
            let found = Section::of_heading(&t[level..].replace("**", ""));
            if level <= 2 {
                section = found.map(|s| (s, level));
                p.seen.extend(found);
                continue;
            }
            if level == 3 && matches!(section, None | Some((_, 3))) {
                if let Some(s) = found {
                    section = Some((s, 3));
                    p.seen.insert(s);
                    continue;
                }
                // A level-3 heading outside the layout closes a level-3 section, unless it is one of the section's items.
                if let Some((s, _)) = section {
                    if item_line(&bare(line), s.letter(), true).is_none() {
                        section = None;
                        continue;
                    }
                }
            }
        }
        let Some((s, _)) = section else { continue };
        let b = bare(line);
        let marked = heading || after_marker(t).is_some();
        match item_line(&b, s.letter(), marked) {
            Some((id, Some((refs, rest)))) => {
                p.items.entry(s).or_default().push(Item { id, refs, fields: fields_in(&rest).into_iter().collect() });
                open = Some(Open { indent, heading, blank: false });
                continue;
            }
            Some((id, None)) => {
                p.unread.push((s, id, line.trim().to_string()));
                open = None;
                continue;
            }
            None => {}
        }
        let Some(o) = open.as_mut() else { continue };
        if t.is_empty() {
            o.blank |= !o.heading;
            continue;
        }
        let under = indent > o.indent || (!o.blank && starts_with_field(&b));
        if under {
            o.blank = false;
            if let Some(last) = p.items.get_mut(&s).and_then(|v| v.last_mut()) {
                last.fields.extend(fields_in(&b));
            }
        } else if !o.heading {
            open = None;
        }
    }
    p
}

/// A document passed whole inside one code fence, as a model often sends it, without that fence; any other text as it is.
pub(crate) fn unfence(text: &str) -> &str {
    let t = text.trim();
    let Some((open, rest)) = t.split_once('\n') else { return text };
    let open = open.trim();
    let Some(c) = open.chars().next().filter(|c| matches!(c, '`' | '~')) else { return text };
    let n = open.chars().take_while(|x| *x == c).count();
    // A backtick fence's info string has no backtick (```` ```toml``` ```` is inline code, not a fence).
    if n < 3 || (c == '`' && open[n..].contains('`')) {
        return text;
    }
    let Some((body, close)) = rest.rsplit_once('\n') else { return text };
    let is_close = |l: &str| {
        let l = l.trim();
        l.len() >= n && l.chars().all(|x| x == c)
    };
    // A closing mark inside means the text is several blocks, not one wrapped document.
    if !is_close(close) || body.lines().any(is_close) {
        return text;
    }
    body
}

/// The findings of a strategy document, and notes that are not findings (an action with no owner or measure).
pub fn trace(text: &str) -> (Vec<Finding>, Vec<String>) {
    let Parsed { items, seen, unread } = parse(text);
    let of = |s: Section| items.get(&s).map(|v| v.as_slice()).unwrap_or(&[]);
    let ids = |s: Section| of(s).iter().map(|i| i.id.as_str()).collect::<BTreeSet<_>>();
    let (mut findings, mut notes) = (Vec::new(), Vec::new());

    for s in [Section::Diagnosis, Section::Policies, Section::Actions] {
        for (_, id, line) in unread.iter().filter(|(u, _, _)| *u == s) {
            findings.push(Finding::new("TRACE-DOC-02", id, format!("the line `{}` starts with {} but could not be read as an item (write `{}` on one line), so it is not traced", line, id, s.example())));
        }
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
    let kind = |id: &str| [(Section::Diagnosis, "a diagnosis"), (Section::Policies, "a policy"), (Section::Actions, "an action")].into_iter().find(|(s, _)| ids(*s).contains(id)).map(|(_, k)| k);
    // A link holds only when it names an id of the right section; any other id is reported and does not count.
    let mut links = |item: &Item, known: &BTreeSet<&str>, want: &str, code: &'static str, none: &str| {
        let unknown: Vec<&str> = item.refs.iter().map(|r| r.as_str()).filter(|r| !known.contains(r)).collect();
        for r in &unknown {
            let message = match kind(r) {
                Some(k) => format!("names {}, which is {}, not {}", r, k, want),
                None => format!("names {}, which the document does not define", r),
            };
            findings.push(Finding::new("TRACE-REF-01", &item.id, message));
        }
        if unknown.len() == item.refs.len() {
            findings.push(Finding::new(code, &item.id, none.to_string()));
        }
    };
    for p in of(Section::Policies) {
        links(p, &diagnoses, "a diagnosis", "TRACE-POL-01", "unfounded policy: it names no diagnosis claim it answers (write `P1 (D1): …`)");
    }
    let mut carried = BTreeSet::new();
    for a in of(Section::Actions) {
        links(a, &policies, "a policy", "TRACE-ACT-01", "orphan action: it names no guiding policy it carries out (write `A1 (P1): …`)");
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
/// `Forecasts are hard` and a heading `## Forecast` with nothing after it are prose, not a label.
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
/// that belong to it, must carry a date and a tripwire that says something. The label may follow a list marker or a number or be a
/// heading (`### Forecast (2027-03-31): …`). The forecast runs on through lines indented deeper, a `Tripwire:` line (also after a
/// blank line), and the lines of its own paragraph; a heading, another list item or anything else after a blank line ends it.
/// Only a labelled line is seen: a forecast written as a plain sentence is not found.
fn forecasts(text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    // The forecast being read: its line number, its indent, whether a date and a tripwire were found so far, and whether a blank
    // line has followed it.
    let mut open: Option<(usize, usize, bool, bool, bool)> = None;
    let mut close = |open: &mut Option<(usize, usize, bool, bool, bool)>| {
        if let Some((line, _, dated, tripwire, _)) = open.take() {
            let missing: Vec<&str> = [(dated, "no date (write `Forecast (YYYY-MM-DD): …`)"), (tripwire, "no tripwire (write `Tripwire: …`, an observable that would show it wrong)")].into_iter().filter(|(has, _)| !has).map(|(_, what)| what).collect();
            if !missing.is_empty() {
                out.push(Finding::new("TRACE-FOR-01", &format!("forecast, line {}", line), format!("a forecast with {}", missing.join(" and "))));
            }
        }
    };
    let mut fence = Fence::default();
    for (n, line) in text.lines().enumerate() {
        if fence.skip(line) {
            close(&mut open);
            continue;
        }
        let t = line.trim_start();
        let indent = line.len() - t.len();
        let b = bare(line).replace('`', "");
        if let Some(rest) = forecast_label(&b) {
            close(&mut open);
            open = Some((n + 1, indent, has_date(rest), fields_in(rest).contains(&"tripwire"), false));
        } else if let Some((_, at, dated, tripwire, blank)) = open.as_mut() {
            if t.is_empty() {
                *blank = true;
                continue;
            }
            let is_tripwire = b.get(..9).is_some_and(|l| l.eq_ignore_ascii_case("tripwire:"));
            let paragraph = !*blank && after_marker(t).is_none();
            if !t.starts_with('#') && (indent > *at || is_tripwire || paragraph) {
                *blank = false;
                *dated |= has_date(&b);
                *tripwire |= fields_in(&b).contains(&"tripwire");
            } else {
                close(&mut open);
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
            (Some(_), Some(_)) | (None, None) => return Ok(tool_result_error(args_error(path, text).unwrap_or_default())),
            (None, Some(t)) if t.len() > MAX_TEXT_BYTES => return Ok(tool_result_error(format!("`text` is over {} bytes; write the document to a file and pass its `path`", MAX_TEXT_BYTES))),
            (None, Some(t)) => ("the text".to_string(), unfence(t).to_string(), false),
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

/// What is wrong with a call's arguments, if anything: it takes one of `path` and `text`.
fn args_error(path: Option<&str>, text: Option<&str>) -> Option<&'static str> {
    match (path, text) {
        (None, None) => Some("strategy_trace needs a `path` or a `text`"),
        (Some(_), Some(_)) => Some("strategy_trace takes a `path` or a `text`, not both"),
        _ => None,
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
        let with = |forecast: &str| trace(&format!("{}\n## Outlook\nSome prose.\n{}\n\nMore prose with a date, 2027-01-01, and a Tripwire: that belongs to nothing.\n", CLEAN, forecast)).0;
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

    // The probes below are the inputs that showed each defect in the 2026-11-20 review (V1-*), kept as written.
    const P1: &str = "## Guiding policies\n- P1 (D1): p\n## Actions\n- A1 (P1): a\n";

    // V1-1: a field counts only for the item it is written under. A blank line ends a list item, prose ends it, a heading ends it.
    #[test]
    fn a_field_counts_only_under_its_own_item() {
        let after_prose = format!("## Diagnosis\n- D1: x\n  Falsifier: q\n- D2: y\n\nNote. Falsifier: see each claim above.\n{}", P1.replace("(D1)", "(D1, D2)"));
        assert_eq!(codes(&after_prose), ["TRACE-DIA-01 D2"]);
        let after_heading = format!("## Diagnosis\n- D1: x\n### Appendix\nFalsifier: stray\n{}", P1);
        assert_eq!(codes(&after_heading), ["TRACE-DIA-01 D1"]);
        let after_a_line_it_could_not_read = format!("## Diagnosis\n- D1: x\n- D2 (z\n  Falsifier: z\n{}", P1);
        assert!(codes(&after_a_line_it_could_not_read).contains(&"TRACE-DIA-01 D1".to_string()), "{:?}", codes(&after_a_line_it_could_not_read));
        // a field directly under its item still counts: indented, a nested bullet, or a line that starts with the field
        for ok in ["- D1: x\n  Falsifier: q\n", "- D1: x\n  - Falsifier: q\n", "- D1: x\nFalsifier: q\n", "- D1: x\n\n  Falsifier: q\n", "### D1: x\n\nSome context.\n\nFalsifier: q\n"] {
            let text = format!("## Diagnosis\n{}{}", ok, P1);
            assert!(codes(&text).is_empty(), "{:?} gave {:?}", ok, codes(&text));
        }
    }

    // V1-2: item lines in other common forms are read; a line that starts with an id and still cannot be read is reported.
    #[test]
    fn item_lines_in_common_forms_are_read_and_one_that_cannot_be_is_reported() {
        for form in ["1. D1: x", "1) D1: x", "D1 - x", "- D1 – x", "- D1 — x", "- [ ] D1: x", "- [x] D1: x", "- *D1*: x", "- _D1_: x", "- **D1** - x"] {
            let text = format!("## Diagnosis\n{} Falsifier: q\n{}", form, P1);
            assert!(codes(&text).is_empty(), "{:?} gave {:?}", form, codes(&text));
        }
        let unclosed = "## Diagnosis\n- D1: x Falsifier: q\n## Guiding policies\n- P1 (D1: p\n## Actions\n- A1 (P1): a\n";
        let found = trace(unclosed).0;
        assert_eq!(found.iter().map(|f| format!("{} {}", f.code, f.id)).collect::<Vec<_>>(), ["TRACE-DOC-02 P1", "TRACE-DOC-01 ## Guiding policies", "TRACE-REF-01 A1", "TRACE-ACT-01 A1"]);
        assert!(found[0].message.contains("- P1 (D1: p") && found[0].message.contains("could not be read"), "{}", found[0].message);
        // prose that only starts with the letter, or names an id further on, is not an item and not a finding
        let prose = format!("## Diagnosis\n- D1: x Falsifier: q\nDiagnoses drift. D1 is the main one.\n{}", P1);
        assert!(codes(&prose).is_empty(), "{:?}", codes(&prose));
    }

    // V1-3: a numbered or longer heading, or a level-3 heading under a heading that is not part of the layout, opens its section.
    #[test]
    fn a_numbered_longer_or_third_level_heading_opens_its_section() {
        let numbered = CLEAN.replace("## Diagnosis", "## 1. Diagnosis").replace("## Guiding policies", "## 2) Guiding policies").replace("## Actions", "## 3. Actions and owners");
        assert!(codes(&numbered).is_empty(), "{:?}", codes(&numbered));
        let third = format!("# Title\n## Strategy\n{}", CLEAN.replace("## ", "### "));
        assert!(codes(&third).is_empty(), "{:?}", codes(&third));
        // a level-3 heading under an open section of the layout is a subsection, not a new section
        let sub = CLEAN.replace("## Guiding policies\n", "### Actions we ruled out\n- A9: none\n## Guiding policies\n");
        assert!(codes(&sub).is_empty(), "{:?}", codes(&sub));
        // a level-3 section closes at the next level-3 heading that is not one of the layout
        let closed = format!("## Strategy\n{}\n### Risks\n- A7 (P9): not an action\n", CLEAN.replace("## ", "### "));
        assert!(codes(&closed).is_empty(), "{:?}", codes(&closed));
        // words that only start like a section are not one
        assert_eq!(codes(&CLEAN.replace("## Actions", "## Actionsplan")), ["TRACE-DOC-01 ## Actions", "TRACE-POL-02 P1", "TRACE-POL-02 P2"]);
    }

    // V1-6: a document passed as `text` and wrapped whole in one code fence is read.
    #[test]
    fn a_document_wrapped_whole_in_a_fence_is_read() {
        for (open, close) in [("```markdown", "```"), ("```", "```"), ("~~~md", "~~~"), ("````", "````")] {
            let wrapped = format!("\n{}\n{}{}\n", open, CLEAN, close);
            assert!(codes(unfence(&wrapped)).is_empty(), "{}: {:?}", open, codes(unfence(&wrapped)));
        }
        // only a fence around the whole text is taken off; an example fence inside is still skipped
        assert_eq!(unfence(CLEAN), CLEAN);
        let two = format!("```\n{}```\nprose\n```\nmore\n```", CLEAN);
        assert_eq!(unfence(&two), two);
    }

    // V1-7: references written after the colon are references.
    #[test]
    fn references_after_the_colon_are_read() {
        let text = "## Diagnosis\n- D1: x Falsifier: q\n## Guiding policies\n- P1: (D1) p\n## Actions\n- A1: (P1) a\n";
        assert!(codes(text).is_empty(), "{:?}", codes(text));
    }

    // V1-16, V1-18, V1-19: a tripwire after one blank line, a numbered or heading label, and a forecast wrapped onto the next line.
    #[test]
    fn a_forecast_keeps_its_tripwire_across_a_blank_line_wraps_and_may_be_numbered_or_a_heading() {
        for ok in [
            "Forecast (2027-03-31): x\n\nTripwire: y\n",
            "- Forecast (2027-03-31): x\n\n  Tripwire: y\n",
            "Forecast: checkout is fastest\nby 2027-03-31.\nTripwire: y\n",
            "1. Forecast (2027-03-31): x\n   Tripwire: y\n",
            "### Forecast (2027-03-31): x\nTripwire: y\n",
            "  Forecast (2027-03-31): x\n\tTripwire: y\n",
        ] {
            assert!(forecasts(ok).is_empty(), "{:?} gave {:?}", ok, forecasts(ok));
        }
        for (bad, missing) in [("1. Forecast: x\n", "no date"), ("### Forecast (2027-03-31): x\n", "no tripwire"), ("2) Forecast (2027-03-31): x\n", "no tripwire")] {
            let found = forecasts(bad);
            assert_eq!(found.len(), 1, "{:?}: {:?}", bad, found);
            assert!(found[0].message.contains(missing), "{:?}: {}", bad, found[0].message);
        }
        // what ends a forecast: prose after a blank line, another list item, a heading
        for (bad, why) in [
            ("Forecast (2027-03-31): x\n\nSome prose. Tripwire: not this one.\n", "prose after a blank line"),
            ("Forecast (2027-03-31): x\n## Next\nTripwire: under another heading\n", "a heading"),
            ("Forecast: x\n- an item dated 2027-03-31\n  Tripwire: y\n", "a list item after a paragraph"),
        ] {
            assert_eq!(forecasts(bad).len(), 1, "{}: {:?}", why, forecasts(bad));
        }
    }

    // V1-4: a key inside another word (`unfalsifier:`) does not hide the real field later on the line.
    #[test]
    fn every_place_a_field_key_appears_is_tried() {
        let text = format!("## Diagnosis\n- D1: this is unfalsifier: claim. Falsifier: real one\n{}", P1);
        assert!(codes(&text).is_empty(), "{:?}", codes(&text));
    }

    // V1-5: a fence closes only on its own mark; inline code with three backticks opens nothing.
    #[test]
    fn a_fence_closes_only_on_its_own_mark() {
        let inline = format!("```toml``` is the format.\n{}", CLEAN);
        assert!(codes(&inline).is_empty(), "{:?}", codes(&inline));
        let mixed = format!("```\n~~~\n## Diagnosis\n- D9: an example\n```\n{}", CLEAN);
        assert!(codes(&mixed).is_empty(), "{:?}", codes(&mixed));
        let longer = format!("````\n```\n- Forecast: an example\n```\n````\n{}", CLEAN);
        assert!(codes(&longer).is_empty(), "{:?}", codes(&longer));
        assert_eq!(forecasts("```\n~~~\n- Forecast: in the example\n```\n"), []);
    }

    // V1-8: a reference of the wrong kind says so.
    #[test]
    fn a_reference_of_the_wrong_kind_says_what_it_names() {
        let text = CLEAN.replace("- A1 (P1): Cache", "- A1 (D1): Cache");
        let found = trace(&text).0;
        assert_eq!(found[0].code, "TRACE-REF-01");
        assert_eq!(found[0].message, "names D1, which is a diagnosis, not a policy");
        let text = CLEAN.replace("- P1 (D1): Take", "- P1 (A1): Take");
        assert_eq!(trace(&text).0[0].message, "names A1, which is an action, not a diagnosis");
    }

    // V1-9: a range of ids names every id in it.
    #[test]
    fn a_range_of_ids_names_every_id_in_it() {
        let three = "## Diagnosis\n- D1: x Falsifier: q\n## Guiding policies\n- P1 (D1): p\n- P2 (D1): p\n- P3 (D1): p\n## Actions\n";
        for range in ["P1–P3", "P1-P3", "P1 — P3", "P1 - P3"] {
            let text = format!("{}- A1 ({}): a\n", three, range);
            assert!(codes(&text).is_empty(), "{}: {:?}", range, codes(&text));
        }
        // a reversed or very long range, or one across letters, is not expanded
        assert_eq!(refs_of("P3–P1"), ["P3", "P1"]);
        assert_eq!(refs_of("P1–P99"), ["P1", "P99"]);
        assert_eq!(refs_of("D1–P3"), ["D1", "P3"]);
    }

    // V1-12: the error for a call with neither argument says what is missing.
    #[test]
    fn the_argument_errors_say_what_is_wrong() {
        assert_eq!(args_error(None, None), Some("strategy_trace needs a `path` or a `text`"));
        assert_eq!(args_error(Some("a.md"), Some("x")), Some("strategy_trace takes a `path` or a `text`, not both"));
        assert_eq!(args_error(Some("a.md"), None), None);
        assert_eq!(args_error(None, Some("x")), None);
    }

    // V1-14: `P01` is `P1`.
    #[test]
    fn an_id_with_leading_zeros_is_the_same_id() {
        let text = CLEAN.replace("- A1 (P1): Cache", "- A1 (P01): Cache").replace("- D1:", "- D01:");
        assert!(codes(&text).is_empty(), "{:?}", codes(&text));
        assert_eq!(as_id("p007").as_deref(), Some("P7"));
        assert_eq!(as_id("A0").as_deref(), Some("A0"));
    }

    // V1-15: a dash or a placeholder that starts the value says nothing.
    #[test]
    fn a_dash_or_a_placeholder_first_word_is_no_falsifier() {
        for empty in ["Falsifier: —", "Falsifier: –", "Falsifier: TBD later", "Falsifier: tbc.", "Falsifier: todo: ask ops", "Falsifier: ???"] {
            let text = CLEAN.replace("Falsifier: an incident review names one owner.", empty);
            assert_eq!(codes(&text), ["TRACE-DIA-01 D2"], "{}", empty);
        }
        // a value that only starts with a word that is a placeholder on its own still says something
        let text = CLEAN.replace("Falsifier: an incident review names one owner.", "Falsifier: none of the vendors ships by Q3.");
        assert!(codes(&text).is_empty(), "{:?}", codes(&text));
    }

    #[test]
    fn the_same_text_always_gives_the_same_report() {
        let text = CLEAN.replace("- A1 (P1): Cache", "- A1: Cache");
        assert_eq!(report("x", &text), report("x", &text));
    }
}
