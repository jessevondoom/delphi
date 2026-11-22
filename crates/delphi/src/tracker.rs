//! The spec tracker, read (spec 135 §4.1, §5.3). Pure: text in, rows out. The tracker is free prose kept by builders, so this
//! reads only what every slice row shares (the spec, the slice and the last cell, its state) and never writes it.

use std::path::{Path, PathBuf};

/// Where the tracker lives in a checkout.
pub const PATH: &str = "missions/2026-10-23-foundational-design/spec-tracker.md";

pub fn path(repo: &Path) -> PathBuf {
    repo.join(PATH)
}

/// One slice row: `| <spec> | <slice> | ... | <state> |`, or `| <slice> | ... | <state> |` inside a spec's section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub spec: String,
    pub slice: String,
    /// The middle cells, joined (the content of the slice).
    pub content: String,
    pub state: String,
    /// 1-based line in the tracker.
    pub line: usize,
    /// For a row that starts with its slice: the heading whose section gave it its spec.
    pub section: Option<String>,
    /// For a per-slice file (§14.10): its path; `line` is then 0.
    pub file: Option<String>,
}

impl Row {
    /// Where the row is: `line N` of the table, or the per-slice file.
    pub fn place(&self) -> String {
        match &self.file {
            Some(f) => f.clone(),
            None => format!("line {}", self.line),
        }
    }
}

/// The cells of a table line, split on `|` not escaped with `\`. A pipe inside backticks that was not escaped splits a cell too;
/// that only moves text between middle cells, and only the first two and the last are read.
fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let inner = t.strip_prefix('|')?.strip_suffix('|')?;
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    Some(out)
}

/// A spec cell: a spec number such as `135`, `005g` or `028-A1`.
fn is_spec_cell(s: &str) -> bool {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    digits == 3 && s.len() <= 10 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A heading of the tracker and the spec numbers it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub level: usize,
    pub title: String,
    /// 1-based line of the heading.
    pub line: usize,
    /// The numbers it names: `094`, `005g`; an amendment `028-A1` names `028`; a range `104 to 110` or `078-081` names each
    /// number in it. Empty when the heading does not say `spec` or `specs` (`Council 013: ...` names no spec).
    pub specs: Vec<String>,
}

/// A markdown heading: its level and title.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&c| c == b'#').count();
    if level == 0 || level > 6 || !line[level..].starts_with(' ') {
        return None;
    }
    Some((level, line[level..].trim()))
}

/// The spec numbers a heading names, sorted, without duplicates.
fn heading_specs(title: &str) -> Vec<String> {
    if !title.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w.eq_ignore_ascii_case("spec") || w.eq_ignore_ascii_case("specs")) {
        return Vec::new();
    }
    // Tokens: exactly three digits, an optional lower-case letter, not inside a longer word or number (a date's `2026`, a
    // path's `/013`, a pull request's `#273`). With the byte range of each.
    let b = title.as_bytes();
    let mut tokens: Vec<(String, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let mut end = i;
        if i - start == 3 && end < b.len() && b[end].is_ascii_lowercase() {
            end += 1;
        }
        let before_ok = start == 0 || !(b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'#' || b[start - 1] == b'/');
        let after_ok = end == b.len() || !b[end].is_ascii_alphanumeric();
        if i - start == 3 && before_ok && after_ok {
            tokens.push((title[start..end].to_string(), start, end));
        }
        i = end.max(i);
    }
    let mut out: Vec<String> = tokens.iter().map(|t| t.0.clone()).collect();
    for w in tokens.windows(2) {
        let between = title[w[0].2..w[1].1].trim();
        if matches!(between, "to" | "-" | "\u{2013}") {
            if let (Ok(a), Ok(z)) = (w[0].0.parse::<u32>(), w[1].0.parse::<u32>()) {
                if a < z && z - a <= 30 {
                    out.extend((a + 1..z).map(|n| format!("{:03}", n)));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every heading of the tracker.
pub fn sections(text: &str) -> Vec<Section> {
    text.lines().enumerate().filter_map(|(i, l)| heading(l).map(|(level, title)| Section { level, title: title.to_string(), line: i + 1, specs: heading_specs(title) })).collect()
}

/// The headings that name a spec number (`094`): where its slice-first rows are looked for.
pub fn sections_for(text: &str, spec_number: &str) -> Vec<Section> {
    sections(text).into_iter().filter(|s| s.specs.iter().any(|n| n.eq_ignore_ascii_case(spec_number))).collect()
}

/// A table's separator line (`| :-- | --: |`).
fn is_separator(c: &[String]) -> bool {
    c.iter().all(|x| !x.is_empty() && x.chars().all(|ch| matches!(ch, ':' | '-')))
}

/// A slice cell without bold or backticks around it (`**S11**`, `` `S11` ``).
fn bare(s: &str) -> String {
    s.trim().trim_matches(|c| c == '*' || c == '`').trim().to_string()
}

/// Every slice row of the tracker, in either of its two shapes:
/// - `| <spec> | <slice> | ... | <state> |`, anywhere: at least four cells, the first a spec number. The older summary table
///   (`| 005c Inline console | yes | ...`) has a title in its first cell, so it is not read this way.
/// - `| <slice> | ... | <state> |`, at least three cells, inside the section of a heading that names the spec. The nearest
///   heading above the row that names any spec decides; a section ends at the next heading of the same or a higher level. A
///   heading that names several specs gives the row to each.
///
/// A table's header line (the line above its separator) and the separator are not rows.
pub fn rows(text: &str) -> Vec<Row> {
    let lines: Vec<&str> = text.lines().collect();
    let parsed: Vec<Option<Vec<String>>> = lines.iter().map(|l| cells(l)).collect();
    let mut stack: Vec<Section> = Vec::new();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some((level, title)) = heading(l) {
            while stack.last().is_some_and(|s| s.level >= level) {
                stack.pop();
            }
            stack.push(Section { level, title: title.to_string(), line: i + 1, specs: heading_specs(title) });
            continue;
        }
        let Some(c) = &parsed[i] else { continue };
        let header = parsed.get(i + 1).and_then(|n| n.as_deref()).is_some_and(is_separator);
        if is_separator(c) || header {
            continue;
        }
        if is_spec_cell(&c[0]) {
            if c.len() < 4 || c[1].is_empty() || c[1].starts_with(':') {
                continue;
            }
            out.push(Row { spec: c[0].clone(), slice: bare(&c[1]), content: c[2..c.len() - 1].join(" | "), state: c[c.len() - 1].clone(), line: i + 1, section: None, file: None });
            continue;
        }
        let slice = bare(&c[0]);
        if c.len() < 3 || slice.is_empty() {
            continue;
        }
        let Some(sec) = stack.iter().rev().find(|s| !s.specs.is_empty()) else { continue };
        for n in &sec.specs {
            out.push(Row { spec: n.clone(), slice: slice.clone(), content: c[1..c.len() - 1].join(" | "), state: c[c.len() - 1].clone(), line: i + 1, section: Some(sec.title.clone()), file: None });
        }
    }
    out
}

/// The rows of one spec, by its number (`135`).
pub fn rows_for<'a>(rows: &'a [Row], spec_number: &str) -> Vec<&'a Row> {
    rows.iter().filter(|r| r.spec.eq_ignore_ascii_case(spec_number)).collect()
}

/// The row of one slice of one spec. The slice id matches whole, ignoring case: `S1` is not `S16`.
pub fn find<'a>(rows: &'a [Row], spec_number: &str, slice: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.spec.eq_ignore_ascii_case(spec_number) && r.slice.eq_ignore_ascii_case(slice.trim()))
}

/// The number a spec stem carries (`135-the-forge...` -> `135`), as the tracker writes it.
pub fn spec_number(stem: &str) -> Option<String> {
    delphi_core::specdoc::spec_number(&format!("{}.md", stem))
}

/// Does a state say the row was not updated for finished work (spec 135 §5.3)?
pub fn state_unfinished(state: &str) -> bool {
    let s = state.trim().to_lowercase();
    ["not built", "not-built", "not started", "approved in advance"].iter().any(|p| s.starts_with(p))
}

/// Does a state say the slice is already merged (a warning when it is claimed, §4.1)?
pub fn state_merged(state: &str) -> bool {
    let s = state.trim().to_lowercase();
    s.starts_with("built and merged") || s.starts_with("merged")
}

// ── One file per slice (§14.10) ─────────────────────────────────────────────────────────────────────────────────────────

/// Where the per-slice files live: `<DIR>/<spec>/<slice>.md`.
pub const DIR: &str = "missions/2026-10-23-foundational-design/tracker";
/// The states a per-slice file may give.
pub const STATES: [&str; 7] = ["not-built", "approved", "built", "merged", "run", "deferred", "superseded"];
const KEYS: [&str; 4] = ["state", "date", "pr", "title"];

/// A per-slice file, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceFile {
    pub spec: String,
    pub slice: String,
    pub state: String,
    pub date: String,
    pub pr: Option<u64>,
    pub title: Option<String>,
    /// The prose after the frontmatter.
    pub notes: String,
    /// The path in the checkout, `/`-separated.
    pub path: String,
}

impl SliceFile {
    /// As a tracker row: the title is its content; the state says the date and the pull request.
    pub fn row(&self) -> Row {
        let pr = self.pr.map(|n| format!(", #{}", n)).unwrap_or_default();
        Row { spec: self.spec.clone(), slice: self.slice.clone(), content: self.title.clone().unwrap_or_default(), state: format!("{} ({}{})", self.state, self.date, pr), line: 0, section: None, file: Some(self.path.clone()) }
    }
}

fn valid_spec_dir(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() == 3 || (b.len() == 4 && b[3].is_ascii_lowercase())) && b[..3].iter().all(u8::is_ascii_digit)
}

/// A slice id: letters, digits, `.` and `-`, starting with a letter or digit, at most 32 characters.
pub fn valid_slice_id(s: &str) -> bool {
    s.len() <= 32 && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Reads a per-slice file from its path in the checkout and its text; every problem is listed.
pub fn parse_slice_file(path: &str, text: &str) -> Result<SliceFile, Vec<String>> {
    let mut errs = Vec::new();
    let rel = path.strip_prefix(DIR).and_then(|r| r.strip_prefix('/')).unwrap_or(path);
    let (spec, slice) = match rel.split('/').collect::<Vec<_>>()[..] {
        [spec, file] => (spec.to_string(), file.strip_suffix(".md").unwrap_or("").to_string()),
        _ => (String::new(), String::new()),
    };
    if !valid_spec_dir(&spec) || slice.is_empty() {
        return Err(vec![format!("a per-slice file lives at {}/<spec number>/<slice>.md (spec 135 §14.10)", DIR)]);
    }
    if !valid_slice_id(&slice) {
        errs.push(format!("`{}` is not a slice id: letters, digits, `.` and `-`, starting with a letter or digit, at most 32 characters", slice));
    }
    let doc = match crate::frontmatter::parse(text) {
        Ok(crate::frontmatter::Doc { frontmatter: Some(m), body }) => (m, body),
        Ok(_) => return Err(vec!["no frontmatter: start with `---`, then `state:` and `date:`, then `---`".into()]),
        Err(e) => return Err(vec![e]),
    };
    let (m, body) = doc;
    for k in m.keys() {
        let k = k.as_str().unwrap_or("?");
        if !KEYS.contains(&k) {
            errs.push(format!("unknown key `{}`: a per-slice file takes only {}", k, KEYS.join(", ")));
        }
    }
    let s = |k: &str| crate::frontmatter::get_str(&m, k).map(str::trim).map(String::from);
    let state = s("state").unwrap_or_default();
    if !STATES.contains(&state.as_str()) {
        errs.push(format!("`state` must be one of {} (it is `{}`)", STATES.join(", "), state));
    }
    let date = s("date").unwrap_or_default();
    if !valid_date(&date) {
        errs.push("`date` must be the day of the last change, YYYY-MM-DD".into());
    }
    let pr = match m.get(serde_yaml::Value::String("pr".into())) {
        None => None,
        Some(v) => match v.as_u64().filter(|n| *n > 0) {
            Some(n) => Some(n),
            None => {
                errs.push("`pr` must be a pull request number".into());
                None
            }
        },
    };
    let title = s("title");
    if crate::frontmatter::has(&m, "title") && title.as_deref().is_none_or(|t| t.is_empty() || t.contains('\n')) {
        errs.push("`title` must be one line of text".into());
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(SliceFile { spec, slice, state, date, pr, title, notes: body.trim().to_string(), path: path.to_string() })
}

/// The per-slice files of a checkout, each read or with its problems, sorted by path.
pub fn slice_files(repo: &Path) -> Vec<(String, Result<SliceFile, Vec<String>>)> {
    let mut out = Vec::new();
    let Ok(specs) = std::fs::read_dir(repo.join(DIR)) else { return out };
    for d in specs.flatten() {
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.flatten() {
            let rel = format!("{}/{}/{}", DIR, d.file_name().to_string_lossy(), f.file_name().to_string_lossy());
            let text = std::fs::read_to_string(f.path()).map_err(|e| vec![format!("cannot read it: {}", e)]);
            out.push((rel.clone(), text.and_then(|t| parse_slice_file(&rel, &t))));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The tracker of a checkout, both shapes.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The table's text (empty when it is missing).
    pub text: String,
    /// The table's rows and the per-slice files' rows; a file wins over a table row of the same spec and slice.
    pub rows: Vec<Row>,
    /// The per-slice files that could not be read, with their problems. Never in `rows`.
    pub bad: Vec<(String, Vec<String>)>,
}

/// Reads the table and the per-slice files (§14.10).
pub fn load(repo: &Path) -> Tracker {
    let text = std::fs::read_to_string(path(repo)).unwrap_or_default();
    let mut files = Vec::new();
    let mut bad = Vec::new();
    for (p, r) in slice_files(repo) {
        match r {
            Ok(f) => files.push(f.row()),
            Err(e) => bad.push((p, e)),
        }
    }
    let mut rows: Vec<Row> = rows(&text).into_iter().filter(|r| find(&files, &r.spec, &r.slice).is_none()).collect();
    rows.extend(files);
    Tracker { text, rows, bad }
}

/// The view of `delphi forge tracker` (§14.10): one table of the rows given, sorted by spec and slice, long cells cut, then
/// the per-slice files that could not be read.
pub fn render(rows: &[&Row], bad: &[(String, Vec<String>)]) -> String {
    let cut = |s: &str, n: usize| if s.chars().count() > n { format!("{}...", s.chars().take(n).collect::<String>()) } else { s.to_string() };
    let cell = |s: &str, n: usize| cut(s, n).replace('|', "\\|").replace('\n', " ");
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| (a.spec.to_lowercase(), a.slice.to_lowercase()).cmp(&(b.spec.to_lowercase(), b.slice.to_lowercase())));
    let mut out = String::from("| Spec | Slice | State | Where | What |\n| :-- | :-- | :-- | :-- | :-- |\n");
    for r in sorted {
        let place = r.file.as_deref().map(|f| f.strip_prefix(DIR).map(|p| format!("tracker{}", p)).unwrap_or(f.to_string())).unwrap_or_else(|| format!("spec-tracker.md:{}", r.line));
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", r.spec, r.slice, cell(&r.state, 80), place, cell(&r.content, 80)));
    }
    for (p, errs) in bad {
        out.push_str(&format!("not read: {}: {}\n", p, errs.join("; ")));
    }
    out
}

/// The `status:` of a spec's frontmatter, without its comment (`approved`, `draft`, ...).
pub fn spec_status(spec_text: &str) -> Option<String> {
    let mut lines = spec_text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for l in lines {
        if l.trim() == "---" {
            return None;
        }
        if let Some(v) = l.strip_prefix("status:") {
            let v = v.split('#').next().unwrap_or("").trim().trim_matches('"').to_string();
            return Some(v);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Spec tracker\n\n| Spec | Approved | Built | Proven | What is left |\n| :-- | :-- | :-- | :-- | :-- |\n| 005c Inline console | yes | yes | tests | none |\n\n| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 135 | F0 | `delphi forge claim\\|release\\|list` | approved in advance on the plan; not built |\n| 135 | F1 | `a|b` unescaped | built (this PR) |\n| 028-A1 | E2 | expand | built and merged (#273) |\n| 132 | C1 | quick wins | not built |\n";

    #[test]
    fn slice_rows_are_read_and_the_summary_table_is_not() {
        let r = rows(SAMPLE);
        assert_eq!(r.iter().map(|x| (x.spec.as_str(), x.slice.as_str())).collect::<Vec<_>>(), vec![("135", "F0"), ("135", "F1"), ("028-A1", "E2"), ("132", "C1")]);
        assert_eq!(r[0].state, "approved in advance on the plan; not built");
        // An unescaped pipe in a middle cell does not move the state.
        assert_eq!(r[1].state, "built (this PR)");
        assert_eq!(r[0].line, 9);
        assert_eq!(rows_for(&r, "135").len(), 2);
    }

    const SECTIONS: &str = "# Spec tracker (2026-10-25)\n\n| 005c Inline console | yes | yes | none |\n\n## Spec 094: the merge flow (2026-11-07, approved)\n\nProse.\n\n| Slice | Content | Hours | State |\n| :-- | :-- | --: | :-- |\n| S1 | the spec | 3 | merged (#561) |\n| **S16** | the commits job | 2 | built (this PR) |\n| `S2a` | the skill | 4 | not built |\n\n### Notes on 094\n\n| S17 | a sub-section row | built |\n\n## Council 013: containment (verdict: `councils/013-verdict.md`)\n\n| Slice | Content | State |\n| :-- | :-- | :-- |\n| S3 | not a spec's row | merged |\n\n## Benchmarks (specs 048, 049 and 028-A1; drafted 2026-10-28)\n\n| E2 | expand | built |\n\n## Host program: specs 104 to 106\n\n| Spec | Slice | Content | State |\n| :-- | :-- | :-- | :-- |\n| 105 | S1 | spec-first | merged |\n| S9 | slice-first under a range | merged |\n";

    #[test]
    fn slice_first_rows_take_the_spec_of_their_section() {
        let r = rows(SECTIONS);
        let s1 = find(&r, "094", "S1").unwrap();
        assert_eq!((s1.line, s1.state.as_str(), s1.content.as_str()), (11, "merged (#561)", "the spec | 3"));
        assert_eq!(s1.section.as_deref(), Some("Spec 094: the merge flow (2026-11-07, approved)"));
        // S1 is not S16, and bold or backticks around the id are not part of it.
        assert_eq!(find(&r, "094", "S16").unwrap().line, 12);
        assert_eq!(find(&r, "094", "s2a").unwrap().state, "not built");
        assert!(find(&r, "094", "S").is_none());
        // A sub-heading that names no spec stays in its parent's section.
        assert_eq!(find(&r, "094", "S17").unwrap().line, 17);
        // The header line of a table is not a row, and the summary table names no spec.
        assert!(find(&r, "094", "Slice").is_none());
        assert!(r.iter().all(|x| !x.slice.starts_with("005c")));
        // A Council's section names no spec: its rows belong to nobody, not to the section above it.
        assert!(r.iter().all(|x| x.slice != "S3"), "{:?}", r);
        // A heading naming several specs gives the row to each; an amendment names its base number.
        for n in ["048", "049", "028"] {
            assert_eq!(find(&r, n, "E2").unwrap().line, 27, "{}", n);
        }
        assert!(find(&r, "094", "E2").is_none());
        // Both shapes in one section: spec-first keeps its own spec; slice-first takes each number of the range.
        assert_eq!(find(&r, "105", "S1").unwrap().section, None);
        assert!(find(&r, "104", "S1").is_none());
        for n in ["104", "105", "106"] {
            assert_eq!(find(&r, n, "S9").unwrap().line, 34, "{}", n);
        }
    }

    #[test]
    fn headings_name_specs_and_not_dates_paths_or_councils() {
        let h = |t: &str| heading_specs(t);
        assert_eq!(h("Spec 094: the core-repo merge flow (2026-11-07, approved)"), vec!["094"]);
        assert_eq!(h("Layered learning and training mode (spec 031, approved, 2026-10-26)"), vec!["031"]);
        assert_eq!(h("Benchmarking, context and memory (specs 048, 049, 050; drafted 2026-10-28)"), vec!["048", "049", "050"]);
        assert_eq!(h("Pictures (specs 029-A1 and 028-A2, approved 2026-10-28)"), vec!["028", "029"]);
        assert_eq!(h("Council specs: evidence (specs 078-081, 2026-11-05)"), vec!["078", "079", "080", "081"]);
        assert_eq!(h("Sharing-layer slices (specs 022\u{2013}024; estimates)"), vec!["022", "023", "024"]);
        assert_eq!(h("The knowledge loop: specs 112 to 114 (2026-11-14, approved)"), vec!["112", "113", "114"]);
        assert_eq!(h("Epic threads (spec 005g)"), vec!["005g"]);
        assert!(h("Council 013: containment increment (verdict: `councils/013-verdict.md`)").is_empty());
        assert!(h("Spec tracker (2026-10-25)").is_empty());
        assert!(h("Spec fold (docs only, 2026-10-30)").is_empty());
        assert!(h("Merged in #273 (spec)").is_empty());
    }

    /// The real tracker: a sample of slices in both shapes is found on the right line, and slices of one section are not found
    /// for another spec.
    #[test]
    fn the_real_tracker_rows_are_found_in_both_shapes() {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(PATH)).unwrap();
        let r = rows(&text);
        let line = |n: &str| text.lines().position(|l| l.trim_start().starts_with(n)).map(|i| i + 1);
        let sections = sections(&text);
        let section_of = |spec: &str| sections.iter().find(|s| s.specs == vec![spec.to_string()]).map(|s| s.title.clone());
        // Slice-first, under the spec's own heading.
        for (spec, slice) in [("094", "S13"), ("094", "S14"), ("094", "S15"), ("094", "S16"), ("094", "S1"), ("093", "B1"), ("093", "B2"), ("031", "L2"), ("041", "S3"), ("092", "S2"), ("089", "P4a")] {
            let row = find(&r, spec, slice).unwrap_or_else(|| panic!("no row for {} {}", spec, slice));
            assert_eq!(row.section, section_of(spec), "{} {}", spec, slice);
            let in_section = sections_for(&text, spec)[0].line;
            assert!(row.line > in_section, "{} {} at line {}, before its heading (line {})", spec, slice, row.line, in_section);
            assert_eq!(bare(&cells(text.lines().nth(row.line - 1).unwrap()).unwrap()[0]), slice);
        }
        // An amendment's heading (`spec 028-A1`) and a range (`specs 022-027`) name their numbers.
        assert!(find(&r, "028", "E2").is_some());
        assert!(find(&r, "024", "B3").is_some());
        // Spec-first rows, as before.
        for (spec, slice) in [("135", "F0"), ("135", "F1"), ("135", "F2"), ("125", "S1"), ("123", "S0")] {
            let row = find(&r, spec, slice).unwrap_or_else(|| panic!("no row for {} {}", spec, slice));
            assert_eq!((row.section.as_deref(), Some(row.line)), (None, line(&format!("| {} | {} |", spec, slice))), "{} {}", spec, slice);
        }
        // No false positives across sections: S16 is 094's alone, 093 has no S slices, 094 has no B slices, S1 is not S16.
        assert!(find(&r, "093", "S16").is_none());
        assert!(find(&r, "093", "S1").is_none());
        assert!(find(&r, "094", "B1").is_none());
        assert_ne!(find(&r, "094", "S1").unwrap().line, find(&r, "094", "S16").unwrap().line);
        assert!(find(&r, "094", "S99").is_none());
        // Header lines are never rows.
        assert!(r.iter().all(|x| !["Slice", "Spec"].contains(&x.slice.as_str())), "a header line was read as a row");
    }

    #[test]
    fn states_are_classified() {
        assert!(state_unfinished("Not built; waits for S0"));
        assert!(state_unfinished("approved in advance on the plan; not built"));
        assert!(!state_unfinished("built (this PR)"));
        assert!(state_merged("built and merged (#731)"));
        assert!(state_merged("merged 2026-11-20"));
        assert!(!state_merged("built, not merged"));
    }

    #[test]
    fn the_frontmatter_status_is_read_without_its_comment() {
        assert_eq!(spec_status("---\nid: x\nstatus: approved   # by the operator\n---\n# x").as_deref(), Some("approved"));
        assert_eq!(spec_status("---\nstatus: draft\n---\n").as_deref(), Some("draft"));
        assert_eq!(spec_status("# no frontmatter\nstatus: approved\n"), None);
        assert_eq!(spec_status("---\nid: x\n---\nstatus: approved\n"), None);
    }

    fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(path(t.path()).parent().unwrap()).unwrap();
        std::fs::write(path(t.path()), SAMPLE).unwrap();
        for (p, text) in files {
            let p = t.path().join(DIR).join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        t
    }

    /// AC-25: a per-slice file is read, wins over the table's row of the same slice, and a bad one is reported, never used.
    #[test]
    fn ac25_a_slice_file_is_read_and_wins_over_the_table() {
        let t = repo_with(&[("135/F0.md", "---\nstate: merged\ndate: 2026-11-21\npr: 818\ntitle: the claims\n---\nBuilt and used.\n"), ("135/E1.md", "---\nstate: built\ndate: 2026-11-22\n---\n"), ("135/E9.md", "---\nstate: finished\ndate: 2026-11-22\n---\n")]);
        let tr = load(t.path());
        let f0 = find(&tr.rows, "135", "f0").unwrap();
        assert_eq!((f0.state.as_str(), f0.content.as_str(), f0.line), ("merged (2026-11-21, #818)", "the claims", 0));
        assert_eq!(f0.place(), format!("{}/135/F0.md", DIR));
        assert_eq!(tr.rows.iter().filter(|r| r.spec == "135" && r.slice.eq_ignore_ascii_case("F0")).count(), 1, "the file replaces the row");
        assert!(state_merged(&f0.state));
        assert_eq!(find(&tr.rows, "135", "E1").unwrap().state, "built (2026-11-22)");
        assert!(find(&tr.rows, "135", "F1").unwrap().file.is_none(), "table rows stay");
        assert!(find(&tr.rows, "135", "E9").is_none());
        assert_eq!(tr.bad.len(), 1);
        assert!(tr.bad[0].0.ends_with("135/E9.md") && tr.bad[0].1[0].contains("`state` must be one of"), "{:?}", tr.bad);
        assert!(state_unfinished(&parse_slice_file(&format!("{}/135/E2.md", DIR), "---\nstate: not-built\ndate: 2026-11-22\n---\n").unwrap().row().state));
        // A checkout with neither the table nor any file has no rows, and no error.
        let empty = tempfile::tempdir().unwrap();
        assert!(load(empty.path()).rows.is_empty());
    }

    #[test]
    fn slice_files_are_parsed_strictly() {
        let p = |path: &str, text: &str| parse_slice_file(&format!("{}/{}", DIR, path), text);
        let f = p("005g/S1.2-b.md", "---\nstate: run\ndate: 2026-11-22\npr: 7\n---\n\nNotes.\n").unwrap();
        assert_eq!((f.spec.as_str(), f.slice.as_str(), f.pr, f.notes.as_str()), ("005g", "S1.2-b", Some(7), "Notes."));
        assert!(p("135/E1.md", "no frontmatter").is_err());
        assert!(p("13/E1.md", "---\nstate: built\ndate: 2026-11-22\n---\n").is_err());
        assert!(p("135/sub/E1.md", "---\nstate: built\ndate: 2026-11-22\n---\n").is_err());
        assert_eq!(p("135/E1.md", "---\nstate: built\ndate: 22-11-2026\npr: -3\ntitle: \"\"\n---\n").unwrap_err().len(), 3);
    }

    /// AC-28: the view has both shapes, says where each row came from, and cuts long cells.
    #[test]
    fn ac28_the_view_shows_both_shapes() {
        let t = repo_with(&[("135/E1.md", "---\nstate: built\ndate: 2026-11-22\ntitle: a|b\n---\n"), ("135/E9.md", "---\nstate: x\n---\n")]);
        let tr = load(t.path());
        let out = render(&rows_for(&tr.rows, "135"), &tr.bad);
        assert!(out.starts_with("| Spec | Slice | State | Where | What |\n"), "{}", out);
        assert!(out.contains("| 135 | E1 | built (2026-11-22) | tracker/135/E1.md | a\\|b |"), "{}", out);
        assert!(out.contains("| 135 | F0 | approved in advance on the plan; not built | spec-tracker.md:9 |"), "{}", out);
        assert!(out.find("| 135 | E1 ").unwrap() < out.find("| 135 | F0 ").unwrap(), "sorted by slice");
        assert!(!out.contains("| 132 |"));
        assert!(out.contains("not read: ") && out.contains("135/E9.md"), "{}", out);
    }

    #[test]
    fn spec_numbers_come_from_stems() {
        assert_eq!(spec_number("135-the-forge").as_deref(), Some("135"));
    }
}
