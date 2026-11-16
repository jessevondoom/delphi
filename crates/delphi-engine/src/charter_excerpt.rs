//! The charter excerpt a host role is given (spec 106 §2, amendments A1 and A2): parts of the ratified `GOVERNANCE.md`, chosen by
//! the fixed table [`SELECTION`] and copied verbatim, in charter order, under the charter's own headings. The only text that is not
//! the charter's is the closing note [`footer`] writes: what was left out (by number and title, taken from the charter's own
//! headings), the charter's hash, that the whole charter governs, how to read it, and the fixed §5.12 sentence of amendment A2.
//!
//! Fail closed: when a selected section or numbered item is not found exactly once (the charter was restructured), [`excerpt`]
//! returns an error and the caller serves the whole charter. A role with no row in the §2 table is not a failure (A1).
//!
//! The scout excerpt of `specs/114` §12 ([`scout_excerpt`]) is a second fixed table, [`SCOUT_SELECTION`], built by the same code
//! with the same note in a short form (no titles, no §5.12 sentence, no way to read the whole). It fails closed the other way: its caller leaves it out, and never serves the whole charter in its place.

/// What is taken from one numbered section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The whole section.
    Whole,
    /// The opening paragraph, the table's header and the row of the role being served (§2).
    RoleRow,
    /// These top-level numbered items, verbatim.
    Items(&'static [u32]),
    /// The opening paragraph only.
    Opening,
}

/// The fixed selection (spec 106 §2 item 1, amended by A2): §1 whole; §2's opening sentence, table header and the served role's row;
/// §3 items 1 and 2; §4 items 1 and 3; §5 items 1 to 5 and 7; §7's opening paragraph. Everything else is left out.
pub const SELECTION: &[(u32, Pick)] = &[(1, Pick::Whole), (2, Pick::RoleRow), (3, Pick::Items(&[1, 2])), (4, Pick::Items(&[1, 3])), (5, Pick::Items(&[1, 2, 3, 4, 5, 7])), (7, Pick::Opening)];

/// The scout excerpt (`specs/114` §12 item 2): §2's opening paragraph, the table's header and the `scout` row; §4 item 3; §5 item 3.
/// Built by the same code and closed by the same note as [`SELECTION`]'s excerpt; carried only in the scout's plan and answer calls.
pub const SCOUT_SELECTION: &[(u32, Pick)] = &[(2, Pick::RoleRow), (4, Pick::Items(&[3])), (5, Pick::Items(&[3]))];

/// The closing note's last sentence: how to read the whole charter. Not in the scout excerpt's short note.
const READ_IN_FULL: &str = " Read it in full with `get_agent` and `detail: \"full\"`, or read the resource `delphi://charter`.";

/// Amendment A2's sentence for §5.12, which is too long to carry. Spec text, copied exactly; no model wrote it.
pub const SECTION_5_12_NOTE: &str = "§5.12: you may request a test authorization and never create, extend or approve one; nothing gives you a right or a reason to seek one or to shape work to fit one. Read the item in full before you call request_test_authorization.";

struct Section<'a> {
    num: Option<u32>,
    title: &'a str,
    heading: &'a str,
    body: Vec<&'a str>,
}

struct Item<'a> {
    num: u32,
    lines: Vec<&'a str>,
}

/// The charter's `## ` sections in order, each body without the `---` rules between sections and without trailing blank lines.
fn sections(charter: &str) -> (Option<&str>, Vec<Section<'_>>) {
    let mut title = None;
    let mut out: Vec<Section> = Vec::new();
    for line in charter.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            let (num, t) = match h.split_once(". ") {
                Some((n, t)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => (n.parse().ok(), t),
                _ => (None, h),
            };
            out.push(Section { num, title: t.trim(), heading: line, body: Vec::new() });
        } else if let Some(s) = out.last_mut() {
            if line.trim() != "---" {
                s.body.push(line);
            }
        } else if title.is_none() && line.starts_with("# ") {
            title = Some(line);
        }
    }
    for s in &mut out {
        trim_blank(&mut s.body);
    }
    (title, out)
}

fn trim_blank(lines: &mut Vec<&str>) {
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
}

/// The number of a top-level numbered item's first line (`12. **Title:** ...`), if it is one.
fn item_number(line: &str) -> Option<u32> {
    let (n, rest) = line.split_once(". ")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) || rest.is_empty() {
        return None;
    }
    n.parse().ok()
}

/// The text before the first item, and the items.
fn items<'a>(body: &[&'a str]) -> (Vec<&'a str>, Vec<Item<'a>>) {
    let mut intro = Vec::new();
    let mut out: Vec<Item> = Vec::new();
    for &line in body {
        match item_number(line) {
            Some(num) => out.push(Item { num, lines: vec![line] }),
            None => match out.last_mut() {
                Some(it) => it.lines.push(line),
                None => intro.push(line),
            },
        }
    }
    for it in &mut out {
        trim_blank(&mut it.lines);
    }
    trim_blank(&mut intro);
    (intro, out)
}

/// An item's title: its leading bold text without the closing colon, as the charter writes it.
fn item_title(it: &Item) -> String {
    let first = it.lines[0].split_once(". ").map(|(_, r)| r).unwrap_or("");
    match first.strip_prefix("**").and_then(|r| r.split_once("**")) {
        Some((t, _)) => t.trim().trim_end_matches(':').trim().to_string(),
        None => crate::mcp_wire::cut_at_char(first, 60).to_string(),
    }
}

/// The paragraph a section opens with: its first lines up to a blank line, a table or a numbered item.
fn opening<'a>(body: &[&'a str]) -> Vec<&'a str> {
    body.iter().take_while(|l| !l.trim().is_empty() && !l.starts_with('|') && item_number(l).is_none()).copied().collect()
}

/// The excerpt for `role`, with its closing note, or why it cannot be built (the caller then serves the whole charter).
/// `charter_hash` is the hash the note gives for the ratified file.
pub fn excerpt(charter: &str, role: &str, charter_hash: &str) -> Result<String, String> {
    excerpt_of(charter, role, charter_hash, SELECTION, false)
}

/// The scout excerpt of `specs/114` §12, with its closing note, or why it cannot be built. The `scout` row is one of its
/// selected parts, so a charter without it is an error here, not A1's case. Its caller fails closed by leaving the excerpt out
/// (§12 item 5), never by serving the whole charter.
///
/// Its note is `footer`'s in its short form, which says what §12 item 2 lists and no more: what was left out (by number, not
/// title, a section's items in one entry), the charter's hash, and that the whole charter governs. The charter's title line is
/// not among the selected parts. The titles, the §5.12 sentence and the way to read the whole charter would put it over the 600
/// tokens §12 item 4 allows (890 with them), and the calls it is carried in have no tools to read with.
pub fn scout_excerpt(charter: &str, charter_hash: &str) -> Result<String, String> {
    excerpt_of(charter, "scout", charter_hash, SCOUT_SELECTION, true)
}

/// Item numbers as the short note names them: `1 and 2`, `1, 2 and 4 to 12` (a run of three or more is `a to b`).
fn runs(nums: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < nums.len() {
        let mut j = i;
        while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
            j += 1;
        }
        if j - i >= 2 {
            parts.push(format!("{} to {}", nums[i], nums[j]));
        } else {
            parts.extend(nums[i..=j].iter().map(|n| n.to_string()));
        }
        i = j + 1;
    }
    match parts.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => parts.concat(),
    }
}

/// The excerpt a fixed `selection` makes of the charter for `role`: what [`excerpt`] and [`scout_excerpt`] share. `short` is the
/// scout excerpt's rule: a §2 table without the role's row is an error, and the note is the short one.
fn excerpt_of(charter: &str, role: &str, charter_hash: &str, selection: &[(u32, Pick)], short: bool) -> Result<String, String> {
    let need_row = short;
    // A part left out, by its number and title, or by its number alone in the short note.
    let named = |n: String, title: &str| if short || title.is_empty() { n } else { format!("{n} {title}") };
    let (title, secs) = sections(charter);
    let mut out: Vec<String> = Vec::new();
    // The charter's title line opens the host excerpt; the scout excerpt's table (`specs/114` §12 item 2) does not select it.
    if let Some(t) = title.filter(|_| !short) {
        out.push(t.to_string());
    }
    let mut left_out: Vec<String> = Vec::new();
    let mut no_row = false;
    let mut has_5_12 = false;
    for (num, _) in selection {
        let n = secs.iter().filter(|s| s.num == Some(*num)).count();
        if n != 1 {
            return Err(format!("§{num} is found {n} times in the charter, not once"));
        }
    }
    for s in &secs {
        let label = match s.num {
            Some(n) => named(format!("§{n}"), s.title),
            None => s.title.to_string(),
        };
        let Some((num, pick)) = s.num.and_then(|n| selection.iter().find(|(m, _)| *m == n)).copied() else {
            left_out.push(label);
            continue;
        };
        let (intro, its) = items(&s.body);
        if num == 5 {
            has_5_12 = its.iter().any(|i| i.num == 12);
        }
        let mut part: Vec<String> = vec![s.heading.to_string(), String::new()];
        match pick {
            Pick::Whole => part.push(s.body.join("\n")),
            Pick::RoleRow => {
                let open = opening(&s.body);
                if open.is_empty() {
                    return Err(format!("§{num} has no opening sentence"));
                }
                let table: Vec<&str> = s.body.iter().skip_while(|l| !l.starts_with('|')).take_while(|l| l.starts_with('|')).copied().collect();
                if table.len() < 2 || !table[1].contains("---") {
                    return Err(format!("§{num} has no table header"));
                }
                let want = format!("| `{role}` |");
                let rows: Vec<&str> = table[2..].iter().filter(|l| l.starts_with(&want)).copied().collect();
                if rows.len() > 1 {
                    return Err(format!("§{num}'s table has {} rows for `{role}`", rows.len()));
                }
                no_row = rows.is_empty();
                if no_row && need_row {
                    return Err(format!("§{num}'s table has no row for `{role}`"));
                }
                part.push(open.join("\n"));
                part.push(String::new());
                part.push(table[..2].join("\n"));
                if let Some(r) = rows.first() {
                    part.push(r.to_string());
                }
                left_out.push(format!("{}: the table rows of the other roles and the text after the table", named(format!("§{num}"), s.title)));
            }
            Pick::Items(want) => {
                let mut kept = Vec::new();
                for w in want {
                    let found: Vec<&Item> = its.iter().filter(|i| i.num == *w).collect();
                    if found.len() != 1 {
                        return Err(format!("§{num}.{w} is found {} times in the charter, not once", found.len()));
                    }
                    kept.push(found[0].lines.join("\n"));
                }
                if !intro.is_empty() {
                    left_out.push(format!("{}: the text before its numbered items", named(format!("§{num}"), s.title)));
                }
                let gone: Vec<&Item> = its.iter().filter(|i| !want.contains(&i.num)).collect();
                if short {
                    if !gone.is_empty() {
                        left_out.push(format!("§{num} items {}", runs(&gone.iter().map(|i| i.num).collect::<Vec<_>>())));
                    }
                } else {
                    left_out.extend(gone.iter().map(|i| format!("§{num}.{} {}", i.num, item_title(i))));
                }
                part.push(kept.join("\n"));
            }
            Pick::Opening => {
                let open = opening(&s.body);
                if open.is_empty() {
                    return Err(format!("§{num} has no opening paragraph"));
                }
                part.push(open.join("\n"));
                let (between, after) = items(&s.body[open.len()..]);
                if !between.is_empty() {
                    left_out.push(format!("{}: the text after its opening paragraph", named(format!("§{num}"), s.title)));
                }
                if short {
                    if !after.is_empty() {
                        left_out.push(format!("§{num} items {}", runs(&after.iter().map(|i| i.num).collect::<Vec<_>>())));
                    }
                } else {
                    left_out.extend(after.iter().map(|i| format!("§{num}.{} {}", i.num, item_title(i))));
                }
            }
        }
        out.push(String::new());
        out.push(part.join("\n"));
    }
    out.push(String::new());
    out.push(footer(role, charter_hash, &left_out, no_row, has_5_12, short));
    Ok(out.join("\n"))
}

/// The one paragraph after the excerpt (spec 106 §2 item 2; A1; A2). `short` is the scout excerpt's form (`specs/114` §12 item 2):
/// the same sentences without the §5.12 sentence and the way to read the whole charter.
fn footer(role: &str, charter_hash: &str, left_out: &[String], no_row: bool, has_5_12: bool, short: bool) -> String {
    let row = if no_row { format!(" The charter's table in §2 has no row for `{role}`.") } else { String::new() };
    let s512 = if has_5_12 && !short { format!(" {SECTION_5_12_NOTE}") } else { String::new() };
    let read = if short { "" } else { READ_IN_FULL };
    format!(
        "This is an excerpt of Delphi's ratified charter, `GOVERNANCE.md` (hash {charter_hash}), selected verbatim by a fixed table in Delphi's code; nothing in it is reworded. Left out here: {}.{row}{s512} The whole charter governs, the parts left out included.{read}",
        if left_out.is_empty() { "nothing".to_string() } else { left_out.join("; ") }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARTER: &str = include_str!("../../../GOVERNANCE.md");

    fn verbatim_lines_of(ex: &str) -> impl Iterator<Item = &str> {
        let note = ex.rfind("\nThis is an excerpt of Delphi's ratified charter").unwrap();
        ex[..note].lines()
    }

    // §2 item 3: the excerpt is built from the repository's own charter; a charter change that breaks the table fails here.
    #[test]
    fn the_excerpt_builds_from_the_repositorys_charter_for_every_shipped_role() {
        for role in ["orchestrator", "architect", "builder", "scout", "investigator", "strategist", "red-team", "writer", "messenger", "validator", "administrator", "assistant", "advocate"] {
            let ex = excerpt(CHARTER, role, "0123456789abcdef").unwrap_or_else(|e| panic!("{role}: the excerpt table no longer fits the charter: {e}"));
            // Every line before the note is a line of the charter, in the charter's order.
            let mut at = 0;
            for line in verbatim_lines_of(&ex).filter(|l| !l.is_empty()) {
                let found = CHARTER[at..].find(&format!("{line}\n")).unwrap_or_else(|| panic!("{role}: not verbatim or out of order: {line}"));
                at += found + line.len();
            }
            assert!(ex.len() < CHARTER.len() / 2, "{role}: the excerpt ({} bytes) is under half the charter ({} bytes)", ex.len(), CHARTER.len());
        }
    }

    #[test]
    fn the_excerpt_carries_the_selected_parts_and_the_note_names_what_was_left_out() {
        let ex = excerpt(CHARTER, "red-team", "0123456789abcdef").unwrap();
        for must in [
            "## 1. Sovereignty & Spec-Driven Law",
            "4. **Drift Remediation:**",
            "Multi-agent systems fail when roles blur",
            "| Role (canonical ID) | Scope | Blind spot | Counterweight |",
            "| `red-team` | Attacks specs, plans and PRs |",
            "1. **Structured Handoffs:**",
            "**Blind Parallel Round 1 (anti-anchoring):**",
            "**Round Cap ($R \\le 3$):**",
            "Personal memory never leaves the operator's machine",
            "**Taint gates actions, not only memory:**",
            "5. **Worktrees Are Not Sandboxes:**",
            "7. **Constitutional Changes Are Red-Tier:**",
            "On mission completion, PR merge or defect recovery",
        ] {
            assert!(ex.contains(must), "missing: {must}");
        }
        for gone in ["| `builder` |", "2. **Central Knowledge Base", "6. **Enforcement Is Code", "9. **Provisional Decisions", "12. **An Advance Authorization", "## 6. Upkeep", "1. **Escaped bug", "The provider-dialect **adapter**", "## Overview"] {
            assert!(!ex.contains(gone), "should be left out: {gone}");
        }
        let note = &ex[ex.rfind("This is an excerpt").unwrap()..];
        assert!(!note.contains('\n'), "one paragraph");
        for must in [
            "hash 0123456789abcdef",
            "Overview",
            "§2 Roles, Blind Spots & Counterweights: the table rows of the other roles",
            "§4.2 Central Knowledge Base (`context/` and `missions/`)",
            "§5.6 Enforcement Is Code, Not Prose",
            "§5.12 An Advance Authorization Is a Third Kind of Red Answer",
            "§6 Upkeep & the Rule of 25",
            "§7.4 Role coaching → private memory",
            SECTION_5_12_NOTE,
            "The whole charter governs",
            "`detail: \"full\"`",
            "`delphi://charter`",
        ] {
            assert!(note.contains(must), "the note lacks: {must}\n{note}");
        }
        assert!(!note.contains("§1 ") && !note.contains("§3.") && !note.contains("§5.7"), "nothing selected is listed as left out: {note}");
        assert!(!note.contains("has no row"));
        assert_eq!(ex, excerpt(CHARTER, "red-team", "0123456789abcdef").unwrap(), "byte-stable");
    }

    // specs/114 §12 item 2: the scout excerpt is §2's opening, header and `scout` row, §4 item 3 and §5 item 3, verbatim and in
    // charter order, with the closing note in its short form; nothing else of the charter.
    #[test]
    fn the_scout_excerpt_is_its_three_parts_verbatim_with_the_short_closing_note() {
        let ex = scout_excerpt(CHARTER, "0123456789abcdef").unwrap();
        let mut at = 0;
        for line in verbatim_lines_of(&ex).filter(|l| !l.is_empty()) {
            let found = CHARTER[at..].find(&format!("{line}\n")).unwrap_or_else(|| panic!("not verbatim or out of order: {line}"));
            at += found + line.len();
        }
        for must in ["## 2. Roles, Blind Spots & Counterweights", "Multi-agent systems fail when roles blur", "| Role (canonical ID) | Scope | Blind spot | Counterweight |", "| `scout` |", "## 4. Dual-Tier Memory", "3. **Information Flow Control (IFC)", "**Taint is computed, never declared:**", "**Taint gates actions, not only memory:**", "## 5. Security, Safety", "3. **Secret Hygiene:**"] {
            assert!(ex.contains(must), "missing: {must}");
        }
        for gone in ["## 1. Sovereignty", "## 3. Collaborative", "| `builder` |", "1. **Private Episodic Memory", "5. **Worktrees Are Not Sandboxes:**", "## 7. The Four-Channel", "## Overview"] {
            assert!(!ex.contains(gone), "should be left out: {gone}");
        }
        let note = &ex[ex.rfind("This is an excerpt").unwrap()..];
        assert!(!note.contains('\n'), "one paragraph");
        // The short note: what was left out by number, the hash, and that the whole charter governs; no titles, no §5.12 sentence
        // and no way to read the charter, which a call with no tools could not use.
        assert_eq!(note, "This is an excerpt of Delphi's ratified charter, `GOVERNANCE.md` (hash 0123456789abcdef), selected verbatim by a fixed table in Delphi's code; nothing in it is reworded. Left out here: Overview; §1; §2: the table rows of the other roles and the text after the table; §3; §4 items 1 and 2; §5 items 1, 2 and 4 to 12; §6; §7. The whole charter governs, the parts left out included.");
        assert!(!ex.starts_with("# Delphi Governance"), "the charter's title is not in the scout's table");
        assert_eq!((runs(&[1, 2]), runs(&[1, 2, 4, 5, 6, 12]), runs(&[3]), runs(&[1, 3, 5])), ("1 and 2".into(), "1, 2, 4 to 6 and 12".into(), "3".into(), "1, 3 and 5".into()));
        // Its size by the measure of the budget of `specs/114` §2 item 3: under 600 tokens.
        let tokens = crate::agent_registry::est_tokens(&ex);
        assert!(tokens < 600, "{tokens} tokens");
        assert_eq!(ex, scout_excerpt(CHARTER, "0123456789abcdef").unwrap(), "byte-stable");
    }

    // specs/114 §12 item 5: a selected part of the scout excerpt that is missing or found twice fails closed, the `scout` row included.
    #[test]
    fn a_missing_or_repeated_part_of_the_scout_excerpt_fails_closed() {
        for (from, to) in [("3. **Secret Hygiene:**", "- **Secret Hygiene:**"), ("3. **Information Flow Control", "- **Information Flow Control"), ("| `scout` |", "| scout |"), ("## 4. Dual-Tier Memory", "## Dual-Tier Memory"), ("| :--- | :--- | :--- | :--- |\n", "")] {
            assert!(CHARTER.contains(from), "{from}");
            assert!(scout_excerpt(&CHARTER.replacen(from, to, 1), "h").is_err(), "{from} -> {to}");
        }
        assert!(scout_excerpt(&CHARTER.replacen("| `scout` |", "| `scout` | x |\n| `scout` |", 1), "h").is_err());
        // The host excerpt keeps A1: a role with no row still gets it.
        assert!(excerpt(&CHARTER.replacen("| `scout` |", "| scout |", 1), "scout", "h").is_ok());
        // A part the scout excerpt does not select may change: nothing breaks.
        assert!(scout_excerpt(&CHARTER.replacen("2. **Council Mode Scaling Laws:**", "- **Council Mode Scaling Laws:**", 1), "h").is_ok());
    }

    // A1: a role with no row in the table still gets the excerpt, and the note says so.
    #[test]
    fn a_role_with_no_table_row_gets_the_excerpt_and_the_note_says_so() {
        let ex = excerpt(CHARTER, "advocate", "h").unwrap();
        assert!(ex.contains("| Role (canonical ID) |") && !ex.contains("| `advocate` |"));
        assert!(ex.contains("The charter's table in §2 has no row for `advocate`."));
        assert!(!ex.contains("| `orchestrator` |"));
    }

    // §2 item 3: a selected part that is missing or found twice fails closed.
    #[test]
    fn a_missing_or_repeated_selected_part_fails_closed() {
        let cases = [
            ("## 3. Collaborative Protocol & Constructive Friction", "## Collaborative Protocol"),
            ("2. **Council Mode Scaling Laws:**", "- **Council Mode Scaling Laws:**"),
            ("7. **Constitutional Changes Are Red-Tier:**", "6. **Constitutional Changes Are Red-Tier:**"),
            ("| :--- | :--- | :--- | :--- |\n", ""),
            ("On mission completion, PR merge", "1. On mission completion, PR merge"),
        ];
        for (from, to) in cases {
            assert!(CHARTER.contains(from), "{from}");
            let broken = CHARTER.replacen(from, to, 1);
            assert!(excerpt(&broken, "architect", "h").is_err(), "{from} -> {to}");
        }
        let doubled = format!("{CHARTER}\n## 5. Again\n\n1. **x**\n");
        assert!(excerpt(&doubled, "architect", "h").is_err());
        let two_rows = CHARTER.replacen("| `architect` |", "| `architect` | x |\n| `architect` |", 1);
        assert!(excerpt(&two_rows, "architect", "h").is_err());
        // An item that is not selected may go: nothing breaks.
        assert!(excerpt(&CHARTER.replacen("12. **An Advance", "- **An Advance", 1), "architect", "h").is_ok());
    }
}
