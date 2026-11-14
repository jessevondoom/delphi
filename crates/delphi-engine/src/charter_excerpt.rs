//! The charter excerpt a host role is given (spec 106 §2, amendments A1 and A2): parts of the ratified `GOVERNANCE.md`, chosen by
//! the fixed table [`SELECTION`] and copied verbatim, in charter order, under the charter's own headings. The only text that is not
//! the charter's is the closing note [`footer`] writes: what was left out (by number and title, taken from the charter's own
//! headings), the charter's hash, that the whole charter governs, how to read it, and the fixed §5.12 sentence of amendment A2.
//!
//! Fail closed: when a selected section or numbered item is not found exactly once (the charter was restructured), [`excerpt`]
//! returns an error and the caller serves the whole charter. A role with no row in the §2 table is not a failure (A1).

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
    let (title, secs) = sections(charter);
    let mut out: Vec<String> = Vec::new();
    if let Some(t) = title {
        out.push(t.to_string());
    }
    let mut left_out: Vec<String> = Vec::new();
    let mut no_row = false;
    let mut has_5_12 = false;
    for (num, _) in SELECTION {
        let n = secs.iter().filter(|s| s.num == Some(*num)).count();
        if n != 1 {
            return Err(format!("§{num} is found {n} times in the charter, not once"));
        }
    }
    for s in &secs {
        let label = match s.num {
            Some(n) => format!("§{n} {}", s.title),
            None => s.title.to_string(),
        };
        let Some((num, pick)) = s.num.and_then(|n| SELECTION.iter().find(|(m, _)| *m == n)).copied() else {
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
                part.push(open.join("\n"));
                part.push(String::new());
                part.push(table[..2].join("\n"));
                if let Some(r) = rows.first() {
                    part.push(r.to_string());
                }
                left_out.push(format!("§{num} {}: the table rows of the other roles and the text after the table", s.title));
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
                    left_out.push(format!("§{num} {}: the text before its numbered items", s.title));
                }
                for i in its.iter().filter(|i| !want.contains(&i.num)) {
                    left_out.push(format!("§{num}.{} {}", i.num, item_title(i)));
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
                    left_out.push(format!("§{num} {}: the text after its opening paragraph", s.title));
                }
                for i in &after {
                    left_out.push(format!("§{num}.{} {}", i.num, item_title(i)));
                }
            }
        }
        out.push(String::new());
        out.push(part.join("\n"));
    }
    out.push(String::new());
    out.push(footer(role, charter_hash, &left_out, no_row, has_5_12));
    Ok(out.join("\n"))
}

/// The one paragraph after the excerpt (spec 106 §2 item 2; A1; A2).
fn footer(role: &str, charter_hash: &str, left_out: &[String], no_row: bool, has_5_12: bool) -> String {
    let row = if no_row { format!(" The charter's table in §2 has no row for `{role}`.") } else { String::new() };
    let s512 = if has_5_12 { format!(" {SECTION_5_12_NOTE}") } else { String::new() };
    format!(
        "This is an excerpt of Delphi's ratified charter, `GOVERNANCE.md` (hash {charter_hash}), selected verbatim by a fixed table in Delphi's code; nothing in it is reworded. Left out here: {}.{row}{s512} The whole charter governs, the parts left out included. Read it in full with `get_agent` and `detail: \"full\"`, or read the resource `delphi://charter`.",
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
