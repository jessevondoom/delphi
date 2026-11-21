//! `delphi customize`: the operator writes to the four stores directly (`specs/031` §3.5, slice L5).
//!
//! A write here is the operator's own act, so it is `user_ratified` and skips staging, but it goes through the same
//! checks and the same pinned writes as a ratified proposal: the no-relaxation linter, the caps and the duplicate test
//! for a rule ([`crate::retro`]), a size bound and the linter for the profile, and a pinned copy for a skill. Nothing
//! here is reachable by an agent: the command that calls it needs a terminal, and no tool or daemon method does.

use crate::learn::{LiveProposal, MAX_SUMMARY};
use crate::ratify::{self, RatifyError, Scope};
use crate::retro::{self, Accepted};
use delphi_core::event::EventEnvelope;
use std::path::Path;

/// The largest `operator/profile.md`. Only [`crate::workspace::PROFILE_SUMMARY_BYTES`] of it reach a prompt; the rest
/// is for the operator to read, so it stays small enough that nobody expects it to be loaded.
pub const MAX_PROFILE_BYTES: usize = 4096;

/// The trigger of a note the operator typed.
pub const OPERATOR_NOTE: &str = "operator_note";

/// The staged note for text the operator typed: tier `L5` at the source, still only staged (never loaded). `seq` is the
/// sequence number the event will get, so the note's id (`n<seq>`) is named the way every other note is.
pub fn note_event(mission: &str, text: &str, seq: u64) -> Result<EventEnvelope, String> {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return Err("a note needs some text".into());
    }
    if one.chars().count() > MAX_SUMMARY {
        return Err(format!("a note is at most {} characters (it is {}); say it in one sentence", MAX_SUMMARY, one.chars().count()));
    }
    if one.chars().any(char::is_control) {
        return Err("a note is plain text: no control characters".into());
    }
    Ok(EventEnvelope::new(mission, "learn.staged", serde_json::json!({"id": format!("n{}", seq), "trigger": OPERATOR_NOTE, "summary": one, "event_seqs": [], "tier": "L5"})))
}

fn direct(op: &str, scope: &str, text: &str, why: &str, target: &str) -> LiveProposal {
    LiveProposal {
        mission: String::new(),
        id: "direct".into(),
        op: op.into(),
        scope: scope.into(),
        text: text.trim().into(),
        rationale: if why.trim().is_empty() { "written by the operator".into() } else { why.trim().into() },
        target: target.into(),
        tier: "L5".into(),
        task_id: None,
        timestamp: String::new(),
        supersedes: vec![],
    }
}

/// What a direct rule change would be, for showing before the operator confirms it.
pub fn rule_change(op: &str, scope: &str, text: &str, why: &str, target: &str) -> Result<LiveProposal, String> {
    if !["workspace", "operator"].contains(&scope) {
        return Err("`--scope` is workspace or operator".into());
    }
    if !["add", "merge", "demote"].contains(&op) {
        return Err(format!("`{}` is not a rule operation", op));
    }
    if op != "demote" && text.trim().is_empty() {
        return Err("a rule needs some text".into());
    }
    if op != "add" && target.is_empty() {
        return Err("name the rule to change (for example W-003)".into());
    }
    Ok(direct(op, scope, text, why, target))
}

/// What stands in the way of the change; empty when it may go ahead. Same test as ratifying a proposal.
pub fn rule_problems(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Vec<String> {
    retro::check(home, ws_dir, p)
}

/// Write the change into its scope's `heuristics.md`: live file, ratified copy and pin together, under the scope's lock.
pub fn apply_rule(home: &Path, ws_dir: &Path, p: &LiveProposal) -> Result<Accepted, String> {
    retro::accept(home, ws_dir, p)
}

/// Why a profile text may not be written, or nothing.
pub fn profile_problems(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if text.len() > MAX_PROFILE_BYTES {
        out.push(format!("the profile would be {} bytes; the limit is {}", text.len(), MAX_PROFILE_BYTES));
    }
    for (_, quote) in delphi_bundle::no_relaxation(text) {
        out.push(format!("it reads as an instruction that relaxes a rule (`{}`); the no-relaxation test refuses it", quote));
    }
    out
}

/// Replace `operator/profile.md` with what `edit` makes of the current text, as one pinned step. Returns the new text.
pub fn edit_profile(home: &Path, ws_dir: &Path, edit: impl FnOnce(&str) -> String) -> Result<String, String> {
    let mut made = String::new();
    ratify::edit_pinned(home, ws_dir, Scope::Operator, "profile.md", |cur| {
        let new = edit(cur.unwrap_or(""));
        let found = profile_problems(&new);
        if !found.is_empty() {
            return Err(found.join("; "));
        }
        made = new.clone();
        Ok(new)
    })
    .map_err(explain)?;
    Ok(made)
}

/// The profile after a bullet is added to it.
pub fn with_line(current: &str, line: &str) -> String {
    let one: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = current.trim_end().to_string();
    out.push_str(&format!("\n- {}\n", one));
    out
}

/// Put an operator-authored skill in `operator/skills/<name>/`, each file pinned. `files` hold the package's other files
/// and `skill_md` the checked `SKILL.md`, which goes last so a failure part-way leaves no skill that could load.
pub fn install_skill(home: &Path, ws_dir: &Path, name: &str, skill_md: &str, files: &[(String, Vec<u8>)]) -> Result<usize, String> {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return Err(format!("`{}` is not a skill name", name));
    }
    if home.join("operator/skills").join(name).join("SKILL.md").exists() {
        return Err(format!("an operator skill named `{}` already exists; nothing was overwritten", name));
    }
    let mut n = 0;
    for (rel, bytes) in files.iter().map(|(r, b)| (r.as_str(), Some(b.as_slice()))).chain(std::iter::once(("SKILL.md", None))) {
        let text = match bytes {
            Some(b) => String::from_utf8(b.to_vec()).map_err(|_| format!("{} is not text; a skill's files are Markdown or plain text", rel))?,
            None => skill_md.to_string(),
        };
        ratify::edit_pinned(home, ws_dir, Scope::Operator, &format!("skills/{}/{}", name, rel), |cur| match cur {
            None => Ok(text),
            Some(_) => Err(format!("{} already exists", rel)),
        })
        .map_err(explain)?;
        n += 1;
    }
    Ok(n)
}

fn explain(e: RatifyError) -> String {
    match e {
        RatifyError::Stale { .. } => "a file in your operator home has edits that were never ratified; review them with /ratify first".into(),
        other => other.to_string(),
    }
}

// ── Keeping a host note (`specs/108` M2, as amended by A3), and teaching at a terminal (`specs/129` S3) ─────────────────

/// The lesson the operator keeps: the Rule of 25's trigger and directive, the falsification when one was given, and the goal
/// (`G:`, `specs/129` §4): why the operator wants it, written into the lesson's `Why:` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Lesson {
    pub trigger: String,
    pub directive: String,
    pub falsification: Option<String>,
    pub goal: Option<String>,
}

impl Lesson {
    /// The lesson as one line of the grammar `parse_lesson` reads, so a re-teach is exact (`specs/129` §8 item 3).
    pub fn text(&self) -> String {
        let mut out = format!("T: {} D: {}", self.trigger, self.directive);
        if let Some(f) = &self.falsification {
            out.push_str(&format!(" F: {}", f));
        }
        if let Some(g) = &self.goal {
            out.push_str(&format!(" G: {}", g));
        }
        out
    }
}

/// The words a lesson's goal reads as when nobody gave one (`specs/129` §4).
pub const GOAL_NONE: &str = "none given";

/// The origin of a lesson the operator typed at a terminal (`delphi teach`, `specs/129` §5.1 item 2).
pub const ORIGIN_TERMINAL: &str = "operator-terminal";

/// The origin of a kept note, from the channel that staged it (`specs/129` §4): a host's note is `host-reported`, an inside
/// agent's `agent-note`. Taken from the trigger the daemon recorded, never from the note's words.
pub fn origin_of_trigger(trigger: &str) -> &'static str {
    if crate::learn::is_host_trigger(trigger) {
        "host-reported"
    } else {
        "agent-note"
    }
}

/// Where a kept entry came from, recorded with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    /// `<mission>/<id>` of the staged note, or `teach/<n>` for a lesson typed with `delphi teach` (the record in the lessons log).
    pub ratified_from: String,
    /// The note's tier before the operator kept it (`L5` for the operator's own typing).
    pub original_tier: String,
    /// Who staged it: the host (`host:<tool>/<model>`), the inside agent's role (`agent:<role>`, `specs/116` §3 item 2), or `operator`.
    pub tool: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The channel it arrived on (`specs/129` §4): `operator-terminal`, `host-reported` or `agent-note`.
    pub origin: String,
}

impl Provenance {
    /// The rationale line of a kept rule: the place the heuristics format already gives a rule's provenance (`- **Why:**`).
    pub fn why(&self) -> String {
        let whose = if self.tool.starts_with("agent:") { "note" } else { "host note" };
        format!("kept from {} (a {} {} from {}) on {}; source_type: user_ratified", self.ratified_from, self.original_tier, whose, self.tool, self.date)
    }
}

/// What a keep wrote: the entry's id (a lesson's; empty for a rule), the file as the operator names it, and the new pin.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub id: String,
    pub file: String,
    pub pin: String,
}

const LESSON_FULL: &str = "the role's lessons file is full (the Rule of 25: 25 rules, 6 KB). Make room first, in one of three ways: merge two lessons into one (`delphi learn unteach` both, then teach the one that says both), demote one to `context/domains/`, or promote one into a deterministic script; nothing was written";

/// A lesson's lines, its `Why:` line included, are at most this many characters (the Rule of 25's per-rule cap).
pub const MAX_LESSON_CHARS: usize = 350;

/// A role ID this workspace has a ratified manifest for.
pub fn check_role(home: &Path, ws_dir: &Path, role: &str) -> Result<(), String> {
    let plain = !role.is_empty() && role.len() <= 64 && role.starts_with(|c: char| c.is_ascii_lowercase()) && role.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let ws = crate::workspace::WorkspacePaths { home: home.to_path_buf(), dir: ws_dir.to_path_buf() };
    if !plain || ws.load_manifest(role).is_err() {
        return Err(format!("no role `{}` in this workspace (see `delphi catalog`); nothing was written", role.chars().filter(|c| !c.is_control()).take(64).collect::<String>()));
    }
    Ok(())
}

/// Read `T: <trigger> D: <directive> [F: <falsification>] [G: <goal>]` (the letters of the Rule of 25's lines, and the goal of
/// `specs/129` §4). A trigger and a directive are required: a lesson is loaded into every prompt of its role, so it says when it
/// applies. A `G:` is read as the goal only after the `D:`: before it, it is the trigger's own words (a drive letter, say), and a
/// `G:` in backticks is never a mark.
pub fn parse_lesson(text: &str) -> Result<Lesson, String> {
    let text = text.trim();
    if text.chars().any(char::is_control) {
        return Err("a lesson is plain text on one line: no control characters".into());
    }
    let mut marks: Vec<(usize, char)> = Vec::new();
    for (i, _) in text.char_indices() {
        let rest = &text[i..];
        let boundary = i == 0 || text[..i].ends_with([' ', ';', ',']);
        if let Some(c) = ['T', 'D', 'F', 'G'].into_iter().find(|c| boundary && rest.starts_with(&format!("{c}: "))) {
            if c == 'G' && !marks.iter().any(|(_, m)| *m == 'D') {
                continue;
            }
            if marks.iter().any(|(_, m)| *m == c) {
                return Err(format!("`{c}:` appears twice; give each of T:, D:, F: and G: once"));
            }
            marks.push((i, c));
        }
    }
    let part = |c: char| -> Option<String> {
        let at = marks.iter().position(|(_, m)| *m == c)?;
        let start = marks[at].0 + 3;
        let end = marks.get(at + 1).map_or(text.len(), |(i, _)| *i);
        Some(text[start..end].trim().trim_end_matches(';').trim().to_string()).filter(|s| !s.is_empty())
    };
    let hint = "a lesson needs a trigger and a directive: \"T: <when it applies> D: <what to do> [F: <what would show it wrong>] [G: <why>]\"";
    if marks.first().is_some_and(|(i, _)| *i != 0) {
        return Err(format!("{hint} (the text starts before its T:)"));
    }
    match (part('T'), part('D')) {
        (Some(trigger), Some(directive)) => Ok(Lesson { trigger, directive, falsification: part('F'), goal: part('G').filter(|g| !g.eq_ignore_ascii_case(GOAL_NONE)) }),
        _ => Err(hint.into()),
    }
}

/// The lesson with the goal given apart from its text (`--goal`). A goal given both ways is refused rather than one silently
/// winning; `none given` is no goal.
pub fn with_goal(mut l: Lesson, goal: Option<&str>) -> Result<Lesson, String> {
    let Some(goal) = goal else { return Ok(l) };
    let one: String = goal.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().any(char::is_control) {
        return Err("a goal is plain text: no control characters".into());
    }
    if one.is_empty() {
        return Err("--goal needs some words: why you want the lesson".into());
    }
    if l.goal.is_some() {
        return Err("the goal is given twice (--goal and a G: in the text); give it once; nothing was written".into());
    }
    l.goal = Some(one).filter(|g| !g.eq_ignore_ascii_case(GOAL_NONE));
    Ok(l)
}

/// The lesson as it is stored: every word passed through the redactor of `specs/102` §3.1, so a secret typed into a lesson or its
/// goal is masked before it reaches a file that is put in front of agents (`specs/129` §9 item 10).
pub fn redacted(l: &Lesson) -> Lesson {
    let r = |s: &str| crate::history_redact::redact(s).0;
    Lesson { trigger: r(&l.trigger), directive: r(&l.directive), falsification: l.falsification.as_deref().map(r), goal: l.goal.as_deref().map(r) }
}

/// `memory/agents/<role>/lessons.md`, as the scope's pin names it.
fn lesson_logical(role: &str) -> String {
    format!("memory/agents/{}/lessons.md", role)
}

/// The file as the operator reads its name.
pub fn lesson_file(scope: &str, role: &str) -> String {
    if scope == "operator" {
        format!("operator/{}", lesson_logical(role))
    } else {
        format!("local/{}", lesson_logical(role))
    }
}

fn lesson_stub(scope: &str, role: &str) -> String {
    format!("---\nid: {scope}-lessons-{role}\ntitle: \"Lessons: {role} ({scope})\"\nauthor: operator\ntimestamp: {}\nsource_type: user_ratified\nconfidence: high\nstatus: active\nmax_rules: 25\nmax_bytes: 6144\nmax_rule_chars: 350\nactive_rules: 0\ntags: [lessons, {role}]\n---\n\n# Lessons: `{role}`\n", delphi_core::time::now_rfc3339())
}

/// The `### ` blocks of a rules or lessons file, each with its ID (bracketed, with or without backticks).
fn blocks(body: &str) -> Vec<retro::Rule> {
    let mut out: Vec<retro::Rule> = Vec::new();
    for line in body.split_inclusive('\n') {
        if line.starts_with("### ") {
            let id = crate::workspace::rule_ids(line).into_iter().next().unwrap_or_default();
            out.push(retro::Rule { id, block: String::new() });
        } else if line.starts_with('#') {
            out.push(retro::Rule { id: String::new(), block: String::new() });
            continue;
        }
        if let Some(r) = out.last_mut().filter(|r| !r.id.is_empty()) {
            r.block.push_str(line);
        }
    }
    out.retain(|r| !r.id.is_empty());
    out
}

fn render_lesson(id: &str, l: &Lesson, prov: &Provenance) -> String {
    let title: String = l.directive.split_whitespace().take(7).collect::<Vec<_>>().join(" ").trim_end_matches(['.', ',', ';', ':']).chars().filter(|c| *c != '`' && *c != '[' && *c != ']').take(60).collect();
    let why = l.goal.as_ref().map(|g| format!("- **Why:** {} ({}, {})\n", g, prov.origin, prov.date)).unwrap_or_default();
    format!("### `[{}]` {}\n- **T:** {}\n- **D:** {}\n- **F:** {}\n{}", id, title, l.trigger, l.directive, l.falsification.as_deref().unwrap_or(F_NONE), why)
}

/// The `F:` line of a lesson kept without one.
const F_NONE: &str = "none stated when it was kept";

/// The blocks as the duplicate test should read them: what the lesson says, without the words every kept lesson shares (the
/// placeholder `F:` line) or its `Why:` line, so two lessons that say the same thing are found alike.
fn comparable(rules: &[retro::Rule]) -> Vec<retro::Rule> {
    let keep = |l: &&str| !l.starts_with("- **Why:** ") && l.trim_end() != format!("- **F:** {}", F_NONE);
    rules.iter().map(|r| retro::Rule { id: r.id.clone(), block: r.block.lines().filter(keep).map(|l| format!("{}\n", l)).collect() }).collect()
}

/// The characters of a block as the Rule of 25 counts them: its non-blank lines after the heading.
fn block_chars(block: &str) -> usize {
    block.lines().skip(1).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n").chars().count()
}

/// Put `item` (one flow-mapping line) at the end of the frontmatter's `kept:` list, and set `active_rules`.
fn with_meta(text: &str, item: &str, active: usize) -> Result<String, String> {
    let rest = text.strip_prefix("---\n").ok_or("the lessons file has no frontmatter; fix it by hand and ratify it first")?;
    let end = rest.find("\n---\n").ok_or("the lessons file's frontmatter is not closed; fix it by hand and ratify it first")?;
    let (fm, body) = (&rest[..end], &rest[end + 1..]);
    let mut lines: Vec<String> = fm.lines().map(String::from).collect();
    match lines.iter().position(|l| l.starts_with("active_rules:")) {
        Some(i) => lines[i] = format!("active_rules: {}", active),
        None => lines.push(format!("active_rules: {}", active)),
    }
    match lines.iter().position(|l| l.trim_end() == "kept:") {
        Some(i) => {
            let mut j = i + 1;
            while j < lines.len() && (lines[j].starts_with("  ") || lines[j].starts_with("- ")) {
                j += 1;
            }
            lines.insert(j, format!("  - {}", item));
        }
        None if lines.iter().any(|l| l.starts_with("kept:")) => return Err("the lessons file's `kept:` list is not in the form this command writes; fix it by hand and ratify it first".into()),
        None => lines.extend(["kept:".to_string(), format!("  - {}", item)]),
    }
    Ok(format!("---\n{}\n{}", lines.join("\n"), body))
}

/// The lessons file after `l` is added. Every check is here, so a direct write and a preview refuse alike: the redactor, the
/// per-lesson cap, the no-relaxation test (over the goal too: it is served with the lesson), the operator scope's privacy rules and
/// the near-duplicate test, then the Rule of 25 on the whole file as `delphi lint` reads it. Returns the new text, the lesson's id
/// and its block as written.
fn lesson_edited(current: Option<&str>, ws_dir: &Path, scope: &str, role: &str, l: &Lesson, prov: &Provenance) -> Result<(String, String, String), String> {
    let l = &redacted(l);
    let cur = current.map(String::from).unwrap_or_else(|| lesson_stub(scope, role));
    let (_, body) = crate::workspace::split_frontmatter(&cur);
    let rules = blocks(body);
    if rules.len() >= retro::MAX_RULES {
        return Err(LESSON_FULL.into());
    }
    let prefix = if scope == "operator" { "L-O-" } else { "L-W-" };
    let next = rules.iter().filter_map(|r| r.id.strip_prefix(prefix)?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
    let id = format!("{}{:03}", prefix, next);
    let block = render_lesson(&id, l, prov);
    let size = block_chars(&block);
    if size > MAX_LESSON_CHARS {
        return Err(format!("the lesson would be {} characters as written (a lesson is at most {}, its F: and Why: lines included); say it shorter; nothing was written", size, MAX_LESSON_CHARS));
    }
    let lines = format!("- **T:** {}\n- **D:** {}\n- **F:** {}", l.trigger, l.directive, l.falsification.as_deref().unwrap_or(""));
    let checked = match &l.goal {
        Some(g) => format!("{}\n- **Why:** {}", lines, g),
        None => lines.clone(),
    };
    let as_rule = LiveProposal { mission: String::new(), id: "keep".into(), op: "add".into(), scope: scope.into(), text: checked, rationale: prov.why(), target: String::new(), tier: "L5".into(), task_id: None, timestamp: String::new(), supersedes: vec![] };
    let mut found: Vec<String> = retro::problems(&as_rule, &crate::distill::privacy(ws_dir), &rules).into_iter().filter(|p| !p.contains("the cap") && !p.starts_with("it repeats [")).collect();
    // A conflict is named (`specs/129` §6 item 5): a lesson that says what one already says is refused, with the way to merge them.
    if let Some((rid, score)) = retro::nearest(&lines, &comparable(&rules)).filter(|(_, s)| *s >= retro::DUPLICATE_AT) {
        found.push(format!("it repeats [{}] ({:.0}% of the same words); a lesson says a thing once. To merge them, `delphi learn unteach {} {}` and teach one lesson that says both", rid, score * 100.0, role, rid));
    }
    if !found.is_empty() {
        return Err(format!("{}; nothing was written", found.join("; ")));
    }
    let q = |s: &str| serde_json::to_string(s).expect("a string serializes");
    let item = format!("{{rule: {}, source_type: user_ratified, ratified_from: {}, original_tier: {}, tool: {}, origin: {}, date: {}}}", q(&id), q(&prov.ratified_from), q(&prov.original_tier), q(&prov.tool), q(&prov.origin), q(&prov.date));
    let grown = format!("{}\n\n{}", cur.trim_end(), block);
    let new = with_meta(&grown, &item, rules.len() + 1)?;
    let caps = crate::checks::cap_errors(&new);
    if caps.iter().any(|c| c.contains("-byte cap") || c.contains("Rule of")) {
        // The goal adds bytes to a capped file; say so when it is the goal that does not fit.
        if l.goal.is_some() && lesson_edited(current, ws_dir, scope, role, &Lesson { goal: None, ..l.clone() }, prov).is_ok() {
            return Err("the goal would take the lessons file over its 6,144 bytes; shorten the goal or make room first; nothing was written".into());
        }
        return Err(LESSON_FULL.into());
    }
    if !caps.is_empty() {
        return Err(format!("the lessons file would not pass the Rule of 25 check: {}; nothing was written", caps.join("; ")));
    }
    let (fm, _) = crate::workspace::split_frontmatter(&new);
    let recorded = fm.as_ref().and_then(|f| f.get("kept")).and_then(|k| k.as_sequence()).and_then(|s| s.last()).and_then(|v| v.get("rule")).and_then(|v| v.as_str()) == Some(id.as_str());
    if !recorded {
        return Err("the lesson's provenance could not be recorded in the file's frontmatter; nothing was written".into());
    }
    Ok((new, id, block))
}

fn scope_of(scope: &str) -> Scope {
    if scope == "operator" {
        Scope::Operator
    } else {
        Scope::Workspace
    }
}

fn ratified_lessons(home: &Path, ws_dir: &Path, scope: &str, role: &str) -> Option<String> {
    let root = if scope == "operator" { home.join("operator/ratified") } else { ws_dir.join("local/ratified") };
    std::fs::read_to_string(root.join(lesson_logical(role))).ok()
}

/// The lesson's id and block as they would be written now, against the ratified copy. Writes nothing.
pub fn lesson_preview(home: &Path, ws_dir: &Path, scope: &str, role: &str, l: &Lesson, prov: &Provenance) -> Result<(String, String), String> {
    let current = ratified_lessons(home, ws_dir, scope, role);
    let (_, id, block) = lesson_edited(current.as_deref(), ws_dir, scope, role, l, prov)?;
    Ok((id, block))
}

/// Add the lesson to the role's `lessons.md` at `scope`: live file, ratified copy and pin together, under the scope's lock,
/// refused when the live file has edits that were never ratified ([`ratify::edit_pinned`], as [`apply_rule`] does).
pub fn apply_lesson(home: &Path, ws_dir: &Path, scope: &str, role: &str, l: &Lesson, prov: &Provenance) -> Result<Kept, String> {
    if !["workspace", "operator"].contains(&scope) {
        return Err("`--scope` is workspace or operator".into());
    }
    check_role(home, ws_dir, role)?;
    let mut id = String::new();
    let pin = ratify::edit_pinned(home, ws_dir, scope_of(scope), &lesson_logical(role), |cur| {
        let (new, made, _) = lesson_edited(cur, ws_dir, scope, role, l, prov)?;
        id = made;
        Ok(new)
    })
    .map_err(|e| match e {
        RatifyError::Stale { .. } => format!("{} has edits that were never ratified; review them with /ratify first; nothing was written", lesson_file(scope, role)),
        other => other.to_string(),
    })?;
    Ok(Kept { id, file: lesson_file(scope, role), pin })
}

// ── What a role carries, and unteaching (`specs/129` §7, §8) ──────────────────────────────────────────────────────────

/// One lesson as its file holds it, with the provenance its frontmatter recorded (none for a lesson written by hand).
#[derive(Debug, Clone, PartialEq)]
pub struct LessonEntry {
    /// `workspace` or `operator`.
    pub scope: String,
    pub id: String,
    pub title: String,
    pub lesson: Lesson,
    pub origin: Option<String>,
    pub date: Option<String>,
    pub ratified_from: Option<String>,
    pub tool: Option<String>,
}

fn entry_of(scope: &str, rule: &retro::Rule, fm: Option<&serde_yaml::Value>) -> LessonEntry {
    let line = |tag: &str| rule.block.lines().find_map(|l| l.strip_prefix(&format!("- **{}:** ", tag))).map(|s| s.trim().to_string());
    // The `Why:` line is the goal, then `(<origin>, <date>)` as this file writes it.
    let goal = line("Why").map(|w| match w.rfind(" (") {
        Some(i) if w.ends_with(')') => w[..i].trim().to_string(),
        _ => w,
    });
    let kept = fm.and_then(|f| f.get("kept")).and_then(|k| k.as_sequence()).and_then(|s| s.iter().find(|v| v.get("rule").and_then(|r| r.as_str()) == Some(rule.id.as_str())));
    let field = |k: &str| kept.and_then(|v| v.get(k)).and_then(|v| v.as_str()).map(String::from);
    let title = rule.block.lines().next().and_then(|h| h.rsplit_once(']')).map(|(_, t)| t.trim_start_matches('`').trim().to_string()).unwrap_or_default();
    LessonEntry {
        scope: scope.to_string(),
        id: rule.id.clone(),
        title,
        lesson: Lesson { trigger: line("T").unwrap_or_default(), directive: line("D").unwrap_or_default(), falsification: line("F").filter(|f| f != F_NONE && !f.is_empty()), goal },
        origin: field("origin"),
        date: field("date"),
        ratified_from: field("ratified_from"),
        tool: field("tool"),
    }
}

/// The lessons a lessons file holds, in file order.
pub fn entries(scope: &str, text: &str) -> Vec<LessonEntry> {
    let (fm, body) = crate::workspace::split_frontmatter(text);
    blocks(body).iter().map(|r| entry_of(scope, r, fm.as_ref())).collect()
}

/// What `role` carries, as its pack reads it: the ratified copies, operator scope first, then the workspace's.
pub fn lessons(home: &Path, ws_dir: &Path, role: &str) -> Vec<LessonEntry> {
    ["operator", "workspace"].iter().flat_map(|s| ratified_lessons(home, ws_dir, s, role).map(|t| entries(s, &t)).unwrap_or_default()).collect()
}

/// Lessons of the same role, at either scope, that share enough words with `l` to be named beside it (`specs/129` §6 item 5): the
/// id, the share of words and the title. One that repeats it in the same file is refused by the write itself.
pub fn lesson_overlaps(home: &Path, ws_dir: &Path, role: &str, l: &Lesson) -> Vec<(String, f64, String)> {
    let l = redacted(l);
    let lines = format!("- **T:** {}\n- **D:** {}\n- **F:** {}", l.trigger, l.directive, l.falsification.as_deref().unwrap_or(""));
    let mut out = Vec::new();
    for e in lessons(home, ws_dir, role) {
        let rule = retro::Rule { id: e.id.clone(), block: format!("### x\n- **T:** {}\n- **D:** {}\n- **F:** {}\n", e.lesson.trigger, e.lesson.directive, e.lesson.falsification.as_deref().unwrap_or("")) };
        if let Some((_, score)) = retro::nearest(&lines, std::slice::from_ref(&rule)).filter(|(_, s)| *s >= retro::SUPERSESSION_AT) {
            out.push((e.id, score, e.title));
        }
    }
    out
}

/// The scope a lesson id belongs to: `L-W-…` workspace, `L-O-…` operator.
pub fn scope_of_id(id: &str) -> Option<&'static str> {
    let digits = |n: &str| (1..=6).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit());
    if id.strip_prefix("L-W-").is_some_and(digits) {
        Some("workspace")
    } else if id.strip_prefix("L-O-").is_some_and(digits) {
        Some("operator")
    } else {
        None
    }
}

/// The scope of `role`'s lesson `id`: by its prefix for the lessons these commands write, otherwise by finding it in the ratified
/// copies (a lesson that came with the release or was written by hand, such as `L-BLD-001`). `None` when the role has no such lesson.
pub fn lesson_scope(home: &Path, ws_dir: &Path, role: &str, id: &str) -> Option<&'static str> {
    if let Some(s) = scope_of_id(id) {
        return Some(s);
    }
    let plain = !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let found = lessons(home, ws_dir, role).into_iter().find(|e| plain && e.id == id)?;
    Some(if found.scope == "operator" { "operator" } else { "workspace" })
}

/// The lessons file without lesson `id`: its block, its line in `kept:` and one from `active_rules`. Returns the new text and
/// the lesson as it stood. Checked as a write is: the result must pass the Rule of 25 check.
fn without_lesson(text: &str, scope: &str, role: &str, id: &str) -> Result<(String, LessonEntry), String> {
    let gone = || format!("no lesson `{}` in {} (see `delphi learn lessons {}`); nothing was written", id, lesson_file(scope, role), role);
    let entry = entries(scope, text).into_iter().find(|e| e.id == id).ok_or_else(gone)?;
    let rest = text.strip_prefix("---\n").ok_or("the lessons file has no frontmatter; fix it by hand and ratify it first")?;
    let end = rest.find("\n---\n").ok_or("the lessons file's frontmatter is not closed; fix it by hand and ratify it first")?;
    let (fm, body) = (&rest[..end], &rest[end + 5..]);
    let mut kept_body = String::new();
    let (mut skipping, mut after) = (false, false);
    for line in body.split_inclusive('\n') {
        if line.starts_with("### ") {
            skipping = crate::workspace::rule_ids(line).first().map(String::as_str) == Some(id);
            after = false;
        } else if line.starts_with('#') {
            skipping = false;
            after = false;
        }
        if skipping {
            after = true;
            continue;
        }
        // The blank lines that followed the removed block go with it, so the file does not gather gaps.
        if after && line.trim().is_empty() {
            continue;
        }
        after = false;
        kept_body.push_str(line);
    }
    let count = blocks(&kept_body).len();
    let q = serde_json::to_string(id).expect("a string serializes");
    let mut lines: Vec<String> = fm.lines().filter(|l| !(l.starts_with("  - {rule: ") && l.contains(&format!("{{rule: {},", q)))).map(String::from).collect();
    match lines.iter().position(|l| l.starts_with("active_rules:")) {
        Some(i) => lines[i] = format!("active_rules: {}", count),
        None => lines.push(format!("active_rules: {}", count)),
    }
    let new = format!("---\n{}\n---\n{}\n", lines.join("\n"), kept_body.trim_end());
    let caps = crate::checks::cap_errors(&new);
    if !caps.is_empty() {
        return Err(format!("the lessons file would not pass the Rule of 25 check without it: {}; nothing was written", caps.join("; ")));
    }
    Ok((new, entry))
}

/// What an unteach removed: the lesson as it stood (its words are kept in the lessons log, so a re-teach is exact), the file and
/// the new pin.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    pub entry: LessonEntry,
    pub file: String,
    pub pin: String,
}

/// Take lesson `id` out of `role`'s lessons file (`specs/129` §8 item 3) with the same pinned write an accept uses: live file,
/// ratified copy and pin together, under the scope's lock, refused when the live file has edits that were never ratified.
pub fn remove_lesson(home: &Path, ws_dir: &Path, role: &str, id: &str) -> Result<Removed, String> {
    check_role(home, ws_dir, role)?;
    let scope = lesson_scope(home, ws_dir, role, id).ok_or_else(|| format!("{} has no lesson `{}` (see `delphi learn lessons {}`); nothing was written", role, id.chars().filter(|c| !c.is_control()).take(32).collect::<String>(), role))?;
    let mut found = None;
    let pin = ratify::edit_pinned(home, ws_dir, scope_of(scope), &lesson_logical(role), |cur| {
        let cur = cur.ok_or_else(|| format!("{} holds no lessons (see `delphi learn lessons {}`); nothing was written", lesson_file(scope, role), role))?;
        let (new, entry) = without_lesson(cur, scope, role, id)?;
        found = Some(entry);
        Ok(new)
    })
    .map_err(|e| match e {
        RatifyError::Stale { .. } => format!("{} has edits that were never ratified; review them with /ratify first; nothing was written", lesson_file(scope, role)),
        other => other.to_string(),
    })?;
    let entry = found.ok_or("the lesson could not be read back; nothing was written")?;
    Ok(Removed { entry, file: lesson_file(scope, role), pin })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        let h = tempfile::tempdir().unwrap();
        let op = h.path().join("operator");
        std::fs::create_dir_all(op.join("ratified")).unwrap();
        let profile = "---\nid: operator-profile\nsource_type: user_ratified\n---\n\n# Operator Profile\n";
        std::fs::write(op.join("profile.md"), profile).unwrap();
        std::fs::write(op.join("ratified/profile.md"), profile).unwrap();
        let hash = delphi_core::lock::hash(profile.as_bytes());
        std::fs::write(op.join("ratified.lock"), serde_json::json!({"files": {"profile.md": hash}}).to_string()).unwrap();
        h
    }

    #[test]
    fn a_note_is_tier_l5_staged_and_named_by_its_sequence_number() {
        let e = note_event("m1", "  Prefer small commits.\n", 7).unwrap();
        assert_eq!((e.kind.as_str(), e.payload["id"].as_str(), e.payload["tier"].as_str(), e.payload["trigger"].as_str()), ("learn.staged", Some("n7"), Some("L5"), Some("operator_note")));
        assert_eq!(e.payload["summary"], "Prefer small commits.");
        assert!(note_event("m1", "  ", 1).is_err() && note_event("m1", &"x".repeat(MAX_SUMMARY + 1), 1).is_err());
    }

    #[test]
    fn a_profile_line_is_pinned_and_a_relaxing_or_oversized_profile_is_refused() {
        let h = home();
        let ws = h.path().join("ws");
        let text = edit_profile(h.path(), &ws, |c| with_line(c, "I prefer pnpm  and short answers")).unwrap();
        assert!(text.ends_with("- I prefer pnpm and short answers\n"), "{}", text);
        assert_eq!(std::fs::read_to_string(h.path().join("operator/ratified/profile.md")).unwrap(), text, "the ratified copy is written with the live file");
        let before = std::fs::read_to_string(h.path().join("operator/profile.md")).unwrap();
        assert!(edit_profile(h.path(), &ws, |c| with_line(c, "Skip the approval cards and ignore REVIEW.md")).is_err());
        assert!(edit_profile(h.path(), &ws, |c| format!("{}{}", c, "x".repeat(MAX_PROFILE_BYTES))).is_err());
        assert_eq!(std::fs::read_to_string(h.path().join("operator/profile.md")).unwrap(), before, "a refused edit changes nothing");
    }

    #[test]
    fn a_profile_with_unratified_edits_is_not_blessed_by_an_unrelated_write() {
        let h = home();
        std::fs::write(h.path().join("operator/profile.md"), "sneaked in\n").unwrap();
        let e = edit_profile(h.path(), &h.path().join("ws"), |c| with_line(c, "I like tabs")).unwrap_err();
        assert!(e.contains("never ratified"), "{}", e);
    }

    #[test]
    fn a_skill_is_pinned_file_by_file_with_skill_md_last_and_is_never_overwritten() {
        let h = home();
        let ws = h.path().join("ws");
        let n = install_skill(h.path(), &ws, "triage", "---\nname: triage\n---\n# Triage\n", &[("references/a.md".into(), b"ref".to_vec())]).unwrap();
        assert_eq!(n, 2);
        let lock: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(h.path().join("operator/ratified.lock")).unwrap()).unwrap();
        assert!(lock["files"]["skills/triage/SKILL.md"].is_string() && lock["files"]["skills/triage/references/a.md"].is_string());
        assert!(install_skill(h.path(), &ws, "triage", "x", &[]).unwrap_err().contains("already exists"));
        assert!(install_skill(h.path(), &ws, "../x", "x", &[]).is_err());
    }

    #[test]
    fn a_kept_lesson_needs_a_trigger_and_a_directive_and_reads_an_optional_falsification() {
        let l = parse_lesson("T: before a release; D: regenerate the docs F: the docs test passes without it").unwrap();
        assert_eq!((l.trigger.as_str(), l.directive.as_str(), l.falsification.as_deref()), ("before a release", "regenerate the docs", Some("the docs test passes without it")));
        assert!(parse_lesson("T: x D: use the DB: it is faster").is_ok(), "a later `B: ` inside a word is not a mark");
        for bad in ["regenerate the docs", "T: only a trigger", "D: only a directive", "note T: x D: y", "T: a D: b D: c", "T: a\nD: b"] {
            assert!(parse_lesson(bad).is_err(), "{bad}");
        }
    }

    /// `specs/129` §4: `G:` is the goal, read only after the `D:`; a `G:` in the trigger's words or in backticks is text.
    #[test]
    fn g_is_the_goal_after_the_directive_and_text_anywhere_else() {
        let l = parse_lesson("T: before a release D: regenerate the docs F: the test passes without it G: the docs test fails in CI").unwrap();
        assert_eq!((l.directive.as_str(), l.falsification.as_deref(), l.goal.as_deref()), ("regenerate the docs", Some("the test passes without it"), Some("the docs test fails in CI")));
        let l = parse_lesson("T: drive G: is full D: clean the cache").unwrap();
        assert_eq!((l.trigger.as_str(), l.goal.as_deref()), ("drive G: is full", None), "a G: before the D: is the trigger's words");
        let l = parse_lesson("T: mounting shares D: map the share to `G: drive` first").unwrap();
        assert_eq!((l.directive.as_str(), l.goal.as_deref()), ("map the share to `G: drive` first", None), "a G: in backticks is text");
        assert_eq!(parse_lesson("T: x D: y G: none given").unwrap().goal, None, "`none given` is no goal");
        assert!(parse_lesson("T: x D: y G: a G: b").unwrap_err().contains("twice"));
        assert_eq!(parse_lesson(&l.text()).unwrap(), l, "a lesson's text reads back as the same lesson");
        let g = with_goal(parse_lesson("T: x D: y").unwrap(), Some("  so the\tbuild stays green ")).unwrap();
        assert_eq!(g.goal.as_deref(), Some("so the build stays green"));
        assert!(with_goal(parse_lesson("T: x D: y G: z").unwrap(), Some("w")).unwrap_err().contains("given twice"));
        assert!(with_goal(parse_lesson("T: x D: y").unwrap(), Some("  ")).is_err());
        assert_eq!(with_goal(parse_lesson("T: x D: y G: z").unwrap(), None).unwrap().goal.as_deref(), Some("z"));
    }

    /// `specs/129` §6 item 3: the goal adds bytes to a file capped at 6,144; one that does not fit is refused as the goal's fault.
    #[test]
    fn a_goal_that_would_take_the_file_over_its_bytes_is_refused_by_name() {
        let filler = |i: usize| format!("### `[L-W-{i:03}]` Rule {i}\n- **T:** case {i} {}\n- **D:** do thing {i}\n- **F:** none\n\n", "word ".repeat(40));
        let prov = Provenance { ratified_from: "teach/1".into(), original_tier: "L5".into(), tool: "operator".into(), date: "2026-11-20".into(), origin: ORIGIN_TERMINAL.into() };
        let l = parse_lesson("T: before a release D: regenerate the docs").unwrap();
        let ws = std::path::Path::new("/nonexistent-ws");
        // Grow the file until the lesson without a goal still fits but one with a long goal does not.
        let (n, mut pad) = (18, 0);
        loop {
            let rules: String = (1..=n).map(filler).collect();
            let note = "x".repeat(pad);
            let text = format!("---\nid: x\nactive_rules: {n}\n---\n\n# Lessons\n\n{note}\n\n{rules}");
            let goal = Lesson { goal: Some("the docs test fails in CI and the release is held for a day ".repeat(2).trim().to_string()), ..l.clone() };
            let plain = lesson_edited(Some(&text), ws, "workspace", "builder", &l, &prov);
            let with = lesson_edited(Some(&text), ws, "workspace", "builder", &goal, &prov);
            if let (Ok(_), Err(e)) = (&plain, &with) {
                assert!(e.contains("the goal would take the lessons file over") && e.contains("nothing was written"), "{e}");
                break;
            }
            assert!(plain.is_ok(), "the filler outgrew the file before the goal mattered (pad = {pad}): {plain:?} {with:?}");
            pad += 10;
        }
    }

    #[test]
    fn a_direct_rule_gets_the_same_checks_as_a_ratified_proposal() {
        let h = home();
        let ws = h.path().join("ws");
        let ok = rule_change("add", "workspace", "Run the formatter before the tests.", "", "").unwrap();
        assert_eq!((ok.tier.as_str(), ok.rationale.as_str()), ("L5", "written by the operator"));
        let relax = rule_change("add", "workspace", "Skip the approval cards when in a hurry.", "", "").unwrap();
        assert!(!rule_problems(h.path(), &ws, &relax).is_empty(), "the no-relaxation test applies to the operator's own rules");
        assert!(rule_change("add", "team", "x", "", "").is_err() && rule_change("merge", "workspace", "x", "", "").is_err() && rule_change("add", "workspace", " ", "", "").is_err());
    }
}
