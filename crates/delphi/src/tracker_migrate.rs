//! `delphi forge tracker migrate` (spec 135 §14.14, E4): moves the slice rows of `spec-tracker.md` into per-slice files
//! (§14.10), once. Pure planning over the text (`plan`), then a small writer (`run`). Nothing is lost: each file keeps its
//! row(s) verbatim, and a row leaves the table only when every file it maps to was written.

use crate::tracker::{self, Row};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Which rows to migrate.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only this spec's rows (`094`).
    pub spec: Option<String>,
    /// Only rows under a heading whose title contains this text.
    pub section: Option<String>,
}

/// One per-slice file to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub path: String,
    pub spec: String,
    pub slice: String,
    pub state: String,
    pub date: String,
    pub pr: Option<u64>,
    /// The state cell did not map clearly; `state` is the closest.
    pub unclear: bool,
    /// The table's state cell (of the first row).
    pub cell: String,
    pub text: String,
    /// The table lines (1-based) this file carries.
    pub lines: Vec<usize>,
}

/// A row left in the table, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub line: usize,
    pub spec: String,
    pub slice: String,
    pub why: String,
}

#[derive(Debug, Default)]
pub struct Plan {
    /// Slice rows read (a row under a heading naming several specs counts once per spec).
    pub rows_read: usize,
    /// Rows the reader takes as slices that sit in a table that is not a slice table (left as they are).
    pub other_rows: usize,
    pub files: Vec<Planned>,
    pub skipped: Vec<Skipped>,
    /// The table with the migrated rows removed.
    pub table: String,
}

/// The state vocabulary of §14.10 for a table's state cell, and whether the cell said it clearly (§14.14's table).
pub fn map_state(cell: &str) -> (&'static str, bool) {
    let s = cell.trim().trim_start_matches(['*', '`', '~', ' ']).to_lowercase();
    let starts = |p: &[&str]| p.iter().any(|w| s.starts_with(w));
    let clear = [
        (&["superseded"][..], "superseded"),
        (&["deferred"][..], "deferred"),
        (&["built and merged", "merged"][..], "merged"),
        (&["not built", "not started", "not run"][..], "not-built"),
        (&["approved in advance on the plan; not built"][..], "not-built"),
        (&["run "][..], "run"),
        (&["built"][..], "built"),
        (&["approved", "spec approved"][..], "approved"),
    ];
    if s == "run" || s.starts_with("run,") || s.starts_with("run;") || s.starts_with("run:") || s.starts_with("run (") {
        return ("run", true);
    }
    for (p, state) in clear {
        if starts(p) {
            return (state, true);
        }
    }
    // Not clear: the earliest state word in the cell, else not-built. The cell is kept in the file either way.
    let words = [("superseded", "superseded"), ("deferred", "deferred"), ("merged", "merged"), ("not built", "not-built"), ("not started", "not-built"), ("not approved", "not-built"), ("not done", "not-built"), ("draft", "not-built"), ("built", "built"), ("written", "built"), ("fixed", "built"), ("done", "built"), ("run", "run"), ("approved", "approved")];
    let word_at = |w: &str| s.match_indices(w).map(|(i, _)| i).find(|&i| !s[..i].ends_with(|c: char| c.is_alphanumeric()) && !s[i + w.len()..].starts_with(|c: char| c.is_alphanumeric()));
    let best = words.iter().filter_map(|(w, st)| word_at(w).map(|i| (i, *st))).min_by_key(|(i, _)| *i);
    (best.map(|b| b.1).unwrap_or("not-built"), false)
}

/// The first `YYYY-MM-DD` in a cell.
pub fn date_in(cell: &str) -> Option<String> {
    let b = cell.as_bytes();
    (0..b.len().saturating_sub(9)).find_map(|i| {
        let w = &b[i..i + 10];
        let ok = w.iter().enumerate().all(|(j, c)| if j == 4 || j == 7 { *c == b'-' } else { c.is_ascii_digit() });
        let edge = (i == 0 || !b[i - 1].is_ascii_digit()) && b.get(i + 10).is_none_or(|c| !c.is_ascii_digit());
        (ok && edge).then(|| cell[i..i + 10].to_string())
    })
}

/// The pull request a cell names, when it names exactly one (`#812`).
pub fn one_pr(cell: &str) -> Option<u64> {
    let found: BTreeSet<u64> = cell.split('#').skip(1).filter_map(|r| r.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()).collect();
    (found.len() == 1).then(|| *found.iter().next().expect("one"))
}

/// A table's spec cell and slice cell as a per-slice file's directory and id: `028-A1` + `E2` is `028` + `A1-E2`; `§8` is
/// `sec8`; spaces become `-`. `None` when no valid id comes out.
pub fn file_ids(spec: &str, slice: &str) -> Option<(String, String)> {
    let (spec, prefix) = match spec.split_once('-') {
        Some((n, rest)) => (n.to_string(), format!("{}-", rest)),
        None => (spec.to_string(), String::new()),
    };
    let mut id = prefix;
    for c in slice.trim().chars() {
        match c {
            c if c.is_ascii_alphanumeric() || c == '.' || c == '-' => id.push(c),
            '§' => id.push_str("sec"),
            c if c.is_whitespace() || "/_+,".contains(c) => id.push('-'),
            _ => {}
        }
    }
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let id = id.trim_matches(['-', '.']).to_string();
    let spec_ok = spec.len() >= 3 && spec[..3].bytes().all(|c| c.is_ascii_digit()) && (spec.len() == 3 || (spec.len() == 4 && spec.as_bytes()[3].is_ascii_lowercase()));
    (spec_ok && tracker::valid_slice_id(&id)).then_some((spec, id))
}

fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// The header and separator lines of the table a line is in, with the nearest heading above it.
fn context(lines: &[&str], line: usize) -> (Option<String>, Option<String>) {
    let mut i = line - 1;
    while i > 0 && lines[i - 1].trim_start().starts_with('|') {
        i -= 1;
    }
    let head = (i + 1 < lines.len() && tracker::cells(lines[i + 1]).as_deref().is_some_and(tracker::is_separator)).then(|| format!("{}\n{}", lines[i].trim(), lines[i + 1].trim()));
    let heading = lines[..line - 1].iter().rev().find(|l| tracker::heading(l).is_some()).map(|l| l.trim().to_string());
    (head, heading)
}

/// Is the table a line sits in a slice table: its header starts `| Slice |` or `| Spec | Slice |`? Rows of other tables (a
/// roll-back table, a table of specs) stay where they are.
fn in_slice_table(lines: &[&str], line: usize) -> bool {
    let Some(head) = context(lines, line).0 else { return false };
    let c: Vec<String> = tracker::cells(head.lines().next().unwrap_or("")).unwrap_or_default().iter().map(|x| x.to_lowercase()).collect();
    c.first().is_some_and(|x| x == "slice") || (c.len() > 1 && c[0] == "spec" && c[1] == "slice")
}

/// Plans the migration of `text` (the table). `exists` says whether a per-slice file is already there; `specs` holds the
/// spec numbers of `specs/`; `line_date` gives the day of the last commit that changed a line; `today` the run's date.
pub fn plan(text: &str, f: &Filter, exists: &dyn Fn(&str) -> bool, specs: &BTreeSet<String>, line_date: &dyn Fn(usize) -> Option<String>, today: &str) -> Plan {
    let lines: Vec<&str> = text.lines().collect();
    let mut p = Plan::default();
    let all = tracker::rows(text);
    let heading_of = |r: &Row| context(&lines, r.line).1.unwrap_or_default();
    let wanted: Vec<&Row> = all
        .iter()
        .filter(|r| f.spec.as_ref().is_none_or(|s| r.spec.eq_ignore_ascii_case(s) || r.spec.split('-').next() == Some(s.as_str())))
        .filter(|r| f.section.as_ref().is_none_or(|s| heading_of(r).contains(s.as_str())))
        .collect();
    let chosen: Vec<&Row> = wanted.iter().copied().filter(|r| in_slice_table(&lines, r.line)).collect();
    p.rows_read = chosen.len();
    p.other_rows = wanted.len() - chosen.len();
    // Group by file; the first row decides the state, as the readers took the first row.
    let mut groups: BTreeMap<(String, String), Vec<&Row>> = BTreeMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    for r in &chosen {
        let skip = |why: &str| Skipped { line: r.line, spec: r.spec.clone(), slice: r.slice.clone(), why: why.to_string() };
        let Some((spec, slice)) = file_ids(&r.spec, &r.slice) else {
            p.skipped.push(skip("no valid spec number or slice id comes out of its cells"));
            continue;
        };
        if !specs.contains(&spec) {
            p.skipped.push(skip(&format!("no spec is numbered {}", spec)));
            continue;
        }
        let key = (spec.to_lowercase(), slice.to_lowercase());
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(r);
    }
    for key in order {
        let rows = &groups[&key];
        let (spec, slice) = file_ids(&rows[0].spec, &rows[0].slice).expect("checked");
        let path = format!("{}/{}/{}.md", tracker::DIR, spec, slice);
        if exists(&path) {
            for r in rows {
                p.skipped.push(Skipped { line: r.line, spec: r.spec.clone(), slice: r.slice.clone(), why: format!("{} already exists (it wins over the row)", path) });
            }
            continue;
        }
        p.files.push(file_for(&lines, rows, &spec, &slice, &path, line_date, today));
    }
    // A line leaves the table only when none of its rows was skipped and every one of its rows was written.
    let written: BTreeSet<usize> = p.files.iter().flat_map(|f| f.lines.iter().copied()).collect();
    let kept: BTreeSet<usize> = p.skipped.iter().map(|s| s.line).chain(all.iter().filter(|r| !chosen.iter().any(|c| c.line == r.line && c.spec == r.spec)).map(|r| r.line)).collect();
    let gone: BTreeSet<usize> = written.difference(&kept).copied().collect();
    p.table = rewrite(&lines, &gone, &all, text.ends_with('\n'));
    p
}

fn file_for(lines: &[&str], rows: &[&Row], spec: &str, slice: &str, path: &str, line_date: &dyn Fn(usize) -> Option<String>, today: &str) -> Planned {
    let first = rows[0];
    let (state, clear) = map_state(&first.state);
    let (date, why_date) = match (date_in(&first.state), line_date(first.line)) {
        (Some(d), _) => (d, "the date is the first one in the state cell"),
        (None, Some(d)) => (d, "the date is the day of the last commit that changed the row"),
        (None, None) => (today.to_string(), "the date is the day of the migration: the row has none and no commit was found"),
    };
    let pr = one_pr(&first.state);
    let middle = tracker::cells(lines[first.line - 1]).unwrap_or_default();
    let title_cell = if first.section.is_some() { middle.get(1) } else { middle.get(2) }.filter(|_| middle.len() >= 3).cloned().unwrap_or_default();
    let title: String = title_cell.trim().chars().take(160).collect::<String>() + if title_cell.trim().chars().count() > 160 { "..." } else { "" };
    let mut fm = format!("---\nstate: {}\ndate: {}\n", state, date);
    if let Some(n) = pr {
        fm.push_str(&format!("pr: {}\n", n));
    }
    if !title.is_empty() {
        fm.push_str(&format!("title: {}\n", yaml_str(&title)));
    }
    fm.push_str("---\n\n");
    let mut body = format!("Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on {}; {}.", today, why_date);
    if !clear {
        body.push_str(&format!(" The state cell did not map clearly to a state; `{}` is the closest, and the cell is kept below as it was.", state));
    }
    if !first.slice.eq_ignore_ascii_case(slice) || !first.spec.eq_ignore_ascii_case(spec) {
        body.push_str(&format!(" The table named this slice `{}` of spec `{}`.", first.slice, first.spec));
    }
    if rows.len() > 1 {
        body.push_str(" The table had more than one row for this slice; the first gave the state, and all are kept below.");
    }
    for r in rows {
        let (head, heading) = context(lines, r.line);
        body.push_str(&format!("\n\nThe row (line {}{}):\n\n", r.line, heading.map(|h| format!(", under `{}`", h.trim_start_matches('#').trim().replace('`', "'"))).unwrap_or_default()));
        if let Some(h) = head {
            body.push_str(&h);
            body.push('\n');
        }
        body.push_str(lines[r.line - 1].trim());
    }
    body.push('\n');
    Planned { path: path.to_string(), spec: spec.to_string(), slice: slice.to_string(), state: state.to_string(), date, pr, unclear: !clear, cell: first.state.clone(), text: fm + &body, lines: rows.iter().map(|r| r.line).collect() }
}

/// The table without the lines in `gone`. A table left with no rows is replaced by one line pointing at the view; a table
/// left with some gets that line after it.
fn rewrite(lines: &[&str], gone: &BTreeSet<usize>, all: &[Row], newline: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].trim_start().starts_with('|') {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && lines[i].trim_start().starts_with('|') {
            i += 1;
        }
        let removed: Vec<usize> = (start + 1..=i).filter(|n| gone.contains(n)).collect();
        if removed.is_empty() {
            out.extend(lines[start..i].iter().map(|l| l.to_string()));
            continue;
        }
        let mut specs: Vec<String> = Vec::new();
        for r in all.iter().filter(|r| removed.contains(&r.line)) {
            let n = file_ids(&r.spec, "x").map(|x| x.0).unwrap_or_else(|| r.spec.clone());
            if !specs.contains(&n) {
                specs.push(n);
            }
        }
        let pointer = format!("Slices: per-slice files (spec 135 §14.10); see {}.", specs.iter().map(|s| format!("`delphi forge tracker --spec {}`", s)).collect::<Vec<_>>().join(", "));
        let left: Vec<usize> = (start + 1..=i).filter(|n| !gone.contains(n)).collect();
        let rows_left = left.iter().any(|n| tracker::cells(lines[n - 1]).is_some_and(|c| !tracker::is_separator(&c)) && !(n + 1 <= lines.len() && tracker::cells(lines[*n]).as_deref().is_some_and(tracker::is_separator) && left.contains(&(n + 1))));
        if rows_left {
            out.extend(left.iter().map(|n| lines[n - 1].to_string()));
            out.push(String::new());
        }
        out.push(pointer);
    }
    let mut s = out.join("\n");
    if newline {
        s.push('\n');
    }
    s
}

/// The spec numbers of a checkout's `specs/`.
pub fn spec_numbers(repo: &Path) -> BTreeSet<String> {
    std::fs::read_dir(repo.join("specs")).map(|d| d.flatten().filter_map(|e| delphi_core::specdoc::spec_number(&e.file_name().to_string_lossy())).collect()).unwrap_or_default()
}

/// Per table line, the day of the last commit that changed it (`git blame`, read-only); empty when git cannot say.
pub fn blame_dates(repo: &Path) -> BTreeMap<usize, String> {
    let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(repo).args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let Some(log) = git(&["log", "--format=%H %cs", "--", tracker::PATH]) else { return BTreeMap::new() };
    let days: BTreeMap<&str, &str> = log.lines().filter_map(|l| l.split_once(' ')).collect();
    let Some(blame) = git(&["blame", "--porcelain", "--", tracker::PATH]) else { return BTreeMap::new() };
    let mut out = BTreeMap::new();
    for l in blame.lines() {
        let mut w = l.split(' ');
        if let (Some(sha), Some(_), Some(Ok(line))) = (w.next(), w.next(), w.next().map(str::parse::<usize>)) {
            if sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit()) {
                if let Some(d) = days.get(sha) {
                    out.insert(line, d.to_string());
                }
            }
        }
    }
    out
}

/// Runs the migration on a checkout and says what it did (or, with `dry_run`, would do).
pub fn run(repo: &Path, f: &Filter, dry_run: bool, today: &str) -> Result<String, String> {
    let table = tracker::path(repo);
    let text = std::fs::read_to_string(&table).map_err(|e| format!("cannot read {}: {}", tracker::PATH, e))?;
    let dates = blame_dates(repo);
    let p = plan(&text, f, &|path| repo.join(path).exists(), &spec_numbers(repo), &|n| dates.get(&n).cloned(), today);
    let mut out = String::new();
    for file in &p.files {
        let pr = file.pr.map(|n| format!(" #{}", n)).unwrap_or_default();
        let unclear = if file.unclear { format!(" (closest; the cell said: {})", file.cell.chars().take(100).collect::<String>()) } else { String::new() };
        out.push_str(&format!("{} {}: {} {}{} <- line {}{}\n", if dry_run { "would write" } else { "wrote" }, file.path, file.state, file.date, pr, file.lines.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "), unclear));
        if !dry_run {
            let dest = repo.join(&file.path);
            std::fs::create_dir_all(dest.parent().expect("a parent")).map_err(|e| format!("{}: {}", file.path, e))?;
            std::fs::write(&dest, &file.text).map_err(|e| format!("{}: {}", file.path, e))?;
        }
    }
    for s in &p.skipped {
        out.push_str(&format!("skipped line {} ({} {}): {}\n", s.line, s.spec, s.slice, s.why));
    }
    let removed = text.lines().count() as i64 - p.table.lines().count() as i64;
    if !dry_run && !p.files.is_empty() {
        std::fs::write(&table, &p.table).map_err(|e| format!("{}: {}", tracker::PATH, e))?;
    }
    let unclear = p.files.iter().filter(|f| f.unclear).count();
    out.push_str(&format!(
        "{} slice rows read ({} rows of other tables left as they are); {} files {}; {} rows skipped; {} states mapped by the closest word; the table {} {} lines shorter\n",
        p.rows_read,
        p.other_rows,
        p.files.len(),
        if dry_run { "to write" } else { "written" },
        p.skipped.len(),
        unclear,
        if dry_run { "would be" } else { "is" },
        removed
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# Tracker\n\n## Spec 094: the merge flow (2026-11-07)\n\nProse stays.\n\n| Slice | Content | State |\n| :-- | :-- | :-- |\n| S1 | the spec | merged (#561) |\n| §8 | an amendment | written from the operator's words |\n| S1 | again | built |\n\n## Specs 104 to 105\n\n| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 104 | R1 | jobs | built 2026-11-13 (#700, #701) |\n| 028-A1 | E2 | expand | not started |\n| 999 | Z1 | no such spec | built |\n| 105 | H0 | exists | approved |\n\n## Benchmarks (specs 104, 105)\n\n| Slice | Content | State |\n| :-- | :-- | :-- |\n| B1 | both | deferred to later |\n\n| Feature | Revert | Drill |\n| :-- | :-- | :-- |\n| S4 route | git revert | not run |\n";

    fn specs() -> BTreeSet<String> {
        ["094", "104", "105", "028"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn states_map_and_say_when_they_are_a_guess() {
        assert_eq!(map_state("built and merged (#731)"), ("merged", true));
        assert_eq!(map_state("**merged** 2026-11-20"), ("merged", true));
        assert_eq!(map_state("built (pull request open)"), ("built", true));
        assert_eq!(map_state("approved in advance on the plan; not built"), ("not-built", true));
        assert_eq!(map_state("approved 2026-11-05, not started"), ("approved", true));
        assert_eq!(map_state("Not started"), ("not-built", true));
        assert_eq!(map_state("run 2026-11-21: 3 of 12"), ("run", true));
        assert_eq!(map_state("superseded by spec 111 §10"), ("superseded", true));
        assert_eq!(map_state("partly built"), ("built", false));
        assert_eq!(map_state("draft"), ("not-built", false));
        assert_eq!(map_state("S2a built, S2b merged"), ("built", false));
        assert_eq!(map_state("none"), ("not-built", false));
        assert_eq!(map_state("a re-run live"), ("run", false));
        assert_eq!(map_state("closed unbuilt; not running"), ("not-built", false));
        assert_eq!(map_state("fixed (PR pending)"), ("built", false));
    }

    #[test]
    fn dates_prs_and_ids_are_read_strictly() {
        assert_eq!(date_in("built 2026-11-19-20 (x)").as_deref(), Some("2026-11-19"));
        assert_eq!(date_in("v12026-11-191"), None);
        assert_eq!(one_pr("merged (#561)"), Some(561));
        assert_eq!(one_pr("on main by #724; built in #709"), None);
        assert_eq!(one_pr("built, #12 and #12 again"), Some(12));
        assert_eq!(file_ids("028-A1", "E2"), Some(("028".into(), "A1-E2".into())));
        assert_eq!(file_ids("094", "§8"), Some(("094".into(), "sec8".into())));
        assert_eq!(file_ids("098", "S6 follow-up"), Some(("098".into(), "S6-follow-up".into())));
        assert_eq!(file_ids("098", "a very long slice name that is far too long for an id"), None);
        assert_eq!(file_ids("13", "S1"), None);
    }

    #[test]
    fn a_plan_writes_each_slice_once_keeps_what_it_cannot_and_rewrites_the_table() {
        let exists = |p: &str| p.ends_with("/105/H0.md");
        let p = plan(TABLE, &Filter::default(), &exists, &specs(), &|n| (n == 10).then(|| "2026-11-08".to_string()), "2026-11-23");
        let names: Vec<&str> = p.files.iter().map(|f| f.path.rsplit_once("tracker/").unwrap().1).collect();
        assert_eq!(names, vec!["094/S1.md", "094/sec8.md", "104/R1.md", "028/A1-E2.md", "104/B1.md", "105/B1.md"]);
        assert_eq!(p.rows_read, 9);
        let s1 = &p.files[0];
        assert_eq!((s1.state.as_str(), s1.pr, s1.lines.clone()), ("merged", Some(561), vec![9, 11]));
        assert!(s1.text.starts_with("---\nstate: merged\ndate: 2026-11-23\npr: 561\ntitle: \"the spec\"\n---\n\n"), "{}", s1.text);
        assert!(s1.text.contains("| S1 | again | built |") && s1.text.contains("more than one row"), "{}", s1.text);
        let sec8 = &p.files[1];
        assert_eq!((sec8.state.as_str(), sec8.unclear, sec8.date.as_str()), ("built", true, "2026-11-08"));
        assert!(sec8.text.contains("named this slice `§8`") && sec8.text.contains("| Slice | Content | State |\n| :-- | :-- | :-- |\n| §8 |"), "{}", sec8.text);
        let r1 = &p.files[2];
        assert_eq!((r1.date.as_str(), r1.pr), ("2026-11-13", None));
        assert_eq!(p.files[4].state, "deferred");
        // Every file parses as the lint reads it.
        for f in &p.files {
            assert!(tracker::parse_slice_file(&f.path, &f.text).is_ok(), "{}: {:?}", f.path, tracker::parse_slice_file(&f.path, &f.text));
        }
        let why: Vec<(usize, &str)> = p.skipped.iter().map(|s| (s.line, s.why.as_str())).collect();
        assert_eq!(why.len(), 2);
        assert!(why[0].0 == 19 && why[0].1.contains("999") && why[1].0 == 20 && why[1].1.contains("already exists"), "{:?}", why);
        // The emptied table is one pointer line; the other keeps its unmigrated rows and its header, then the pointer.
        assert!(p.table.contains("Prose stays.\n\nSlices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 094`.\n\n## Specs 104 to 105"), "{}", p.table);
        assert!(p.table.contains("| :-- | :-- | :-- | :-- |\n| 999 | Z1 | no such spec | built |\n| 105 | H0 | exists | approved |\n\nSlices: per-slice files (spec 135 §14.10); see `delphi forge tracker --spec 104`, `delphi forge tracker --spec 028`."), "{}", p.table);
        assert!(p.table.ends_with("see `delphi forge tracker --spec 104`, `delphi forge tracker --spec 105`.\n\n| Feature | Revert | Drill |\n| :-- | :-- | :-- |\n| S4 route | git revert | not run |\n"), "{}", p.table);
        // A row of a table that is not a slice table (a roll-back table) is not moved.
        assert_eq!(p.other_rows, 2);
        assert!(!p.files.iter().any(|f| f.path.contains("S4")));
        assert!(!p.table.contains("| R1 |") && !p.table.contains("| B1 |"));
    }

    #[test]
    fn filters_take_one_spec_or_one_section_and_a_shared_row_stays_until_all_its_specs_move() {
        let none = |_: &str| false;
        let p = plan(TABLE, &Filter { spec: Some("105".into()), section: None }, &none, &specs(), &|_| None, "2026-11-23");
        assert_eq!(p.files.iter().map(|f| f.path.rsplit_once("tracker/").unwrap().1).collect::<Vec<_>>(), vec!["105/H0.md", "105/B1.md"]);
        assert!(p.table.contains("| B1 | both |"), "a row 104 shares stays while 104's file is not written");
        assert!(!p.table.contains("| 105 | H0 |"));
        let p = plan(TABLE, &Filter { spec: None, section: Some("merge flow".into()) }, &none, &specs(), &|_| None, "2026-11-23");
        assert_eq!(p.files.len(), 2);
        assert!(p.table.contains("| 104 | R1 |"));
    }

    #[test]
    fn a_dry_run_writes_nothing_and_a_run_writes_once() {
        let t = tempfile::tempdir().unwrap();
        let table = tracker::path(t.path());
        std::fs::create_dir_all(table.parent().unwrap()).unwrap();
        std::fs::write(&table, TABLE).unwrap();
        std::fs::create_dir_all(t.path().join("specs")).unwrap();
        for s in ["094-a.md", "104-b.md", "105-c.md", "028-d.md"] {
            std::fs::write(t.path().join("specs").join(s), "x").unwrap();
        }
        let out = run(t.path(), &Filter::default(), true, "2026-11-23").unwrap();
        assert!(out.contains("would write") && out.contains("9 slice rows read (2 rows of other tables left as they are); 7 files to write; 1 rows skipped"), "{}", out);
        assert_eq!(std::fs::read_to_string(&table).unwrap(), TABLE);
        assert!(!t.path().join(tracker::DIR).join("094").exists());
        let out = run(t.path(), &Filter::default(), false, "2026-11-23").unwrap();
        assert!(out.contains("7 files written"), "{}", out);
        assert!(tracker::load(t.path()).rows.iter().filter(|r| r.file.is_some()).count() == 7);
        let again = run(t.path(), &Filter::default(), false, "2026-11-23").unwrap();
        assert!(again.contains("0 files written"), "{}", again);
    }
}
