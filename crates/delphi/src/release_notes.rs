//! Release notes from fragments (`specs/036` §9): one file per change under `changelog.d/`, folded into `CHANGELOG.md` by the
//! release pull request, so parallel pull requests never edit the same file.
//!
//! A fragment is `changelog.d/<slug>.md`: a frontmatter block with one line, `section: <Added | Changed | Removed | Fixed |
//! Security>`, then a Markdown list item or items. `delphi release changelog` prints what the next release's section would be;
//! `--write <version>` puts it into `CHANGELOG.md` above the newest release and deletes the fragments; `--check` only reads them.
//! `delphi lint --repo` fails a malformed fragment through [`problems`], so the check runs with the other gates.

use std::fs;
use std::path::{Path, PathBuf};

/// The fragments directory, relative to the repository root.
pub const DIR: &str = "changelog.d";
/// The assembled file, relative to the repository root.
pub const CHANGELOG: &str = "CHANGELOG.md";
/// The one file in [`DIR`] that is not a fragment: it says how to write one.
pub const README: &str = "README.md";
/// The sections, in the order the assembled release lists them.
pub const SECTIONS: [&str; 5] = ["Added", "Changed", "Removed", "Fixed", "Security"];

#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// The file name, without the directory.
    pub name: String,
    pub section: &'static str,
    /// The list item or items, trimmed, with LF line ends.
    pub body: String,
}

/// A fragment's file name: a lowercase slug ending in `.md`.
fn good_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else { return false };
    let mut chars = stem.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// A Markdown link or image whose target is not an absolute `http(s)` address. A fragment is read in `changelog.d/` and then
/// moved to the root, so a relative target cannot be right in both places.
fn relative_link(body: &str) -> Option<String> {
    let mut rest = body;
    while let Some(i) = rest.find("](") {
        let target = &rest[i + 2..];
        let end = target.find(')').unwrap_or(target.len());
        let t = target[..end].trim();
        if !(t.starts_with("https://") || t.starts_with("http://")) {
            return Some(t.to_string());
        }
        rest = &target[end..];
    }
    None
}

/// Reads one fragment. Every rule a fragment can break is named in the error.
pub fn parse(name: &str, text: &str) -> Result<Fragment, String> {
    if !good_name(name) {
        return Err("a fragment's name is a lowercase slug ending in `.md` (letters, digits, `-`, `_`, `.`)".into());
    }
    let text = text.replace("\r\n", "\n");
    let Some(after) = text.strip_prefix("---\n") else {
        return Err("a fragment starts with a frontmatter block (`---`, `section: <name>`, `---`)".into());
    };
    let closed = after.strip_prefix("---\n").map(|b| ("", b)).or_else(|| after.split_once("\n---\n")).or_else(|| after.strip_suffix("\n---").map(|f| (f, "")));
    let Some((front, body)) = closed else {
        return Err("the frontmatter block does not end (a line `---`)".into());
    };
    let mut section = None;
    for line in front.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        match line.split_once(':').map(|(k, v)| (k.trim(), v.trim().trim_matches(|c| c == '"' || c == '\''))) {
            Some(("section", v)) if section.is_none() => match SECTIONS.iter().find(|s| **s == v) {
                Some(s) => section = Some(*s),
                None => return Err(format!("`section: {v}` is not one of {}", SECTIONS.join(", "))),
            },
            Some(("section", _)) => return Err("`section` is given twice".into()),
            _ => return Err(format!("the frontmatter holds one line, `section: <name>`; `{line}` is not it")),
        }
    }
    let Some(section) = section else { return Err(format!("the frontmatter has no `section:` line (one of {})", SECTIONS.join(", "))) };
    let body = body.trim();
    if body.is_empty() {
        return Err("the fragment has no entry below its frontmatter".into());
    }
    if !body.starts_with("- ") {
        return Err("an entry is a Markdown list item: its first line starts with `- `".into());
    }
    if body.lines().any(|l| l.starts_with('#')) {
        return Err("a fragment has no headings; the release's section headings are written when it is assembled".into());
    }
    if let Some(t) = relative_link(body) {
        return Err(format!("a link to `{t}` would break when the entry moves to {CHANGELOG}: link an absolute https address, or name the path in backticks"));
    }
    Ok(Fragment { name: name.to_string(), section, body: body.to_string() })
}

/// What [`DIR`] holds, as (file name, its text or `None` when it is not a readable file). Sorted by name.
fn listing(repo: &Path) -> Result<Vec<(String, Option<String>)>, String> {
    let dir = repo.join(DIR);
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {DIR}/: {e}")),
    };
    let mut out = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {DIR}/: {e}"))?;
        let name = e.file_name().to_string_lossy().to_string();
        let text = if e.file_type().map(|t| t.is_file()).unwrap_or(false) { fs::read_to_string(e.path()).ok() } else { None };
        out.push((name, text));
    }
    out.sort();
    Ok(out)
}

/// Each problem of the fragments in `files` (file name and text, `None` for a directory or an unreadable file), as (file name,
/// why). The lint and the command share it.
pub fn problems(files: &[(String, Option<String>)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, text) in files {
        if name == README {
            continue;
        }
        match text {
            None => out.push((name.clone(), format!("{DIR}/ holds fragment files only, as UTF-8 text, and no directories"))),
            Some(t) => {
                if let Err(e) = parse(name, t) {
                    out.push((name.clone(), e));
                }
            }
        }
    }
    out
}

fn fragments(files: &[(String, Option<String>)]) -> Vec<Fragment> {
    files.iter().filter(|(n, _)| n != README).filter_map(|(n, t)| t.as_deref().and_then(|t| parse(n, t).ok())).collect()
}

/// The release's section: its heading, then each section that has entries, in [`SECTIONS`] order, the entries in file-name order.
pub fn section(version: &str, date: Option<&str>, frags: &[Fragment]) -> String {
    let mut out = match date {
        Some(d) => format!("## {version} — {d}\n"),
        None => format!("## {version}\n"),
    };
    for s in SECTIONS {
        let mut items: Vec<&Fragment> = frags.iter().filter(|f| f.section == s).collect();
        if items.is_empty() {
            continue;
        }
        items.sort_by(|a, b| a.name.cmp(&b.name));
        out.push_str(&format!("\n### {s}\n\n"));
        for f in items {
            out.push_str(&f.body);
            out.push('\n');
        }
    }
    out
}

/// `changelog` with `section` put above its newest release (the first `## ` heading), or at its end when it has none. Refuses a
/// changelog that already has a heading for `version`.
pub fn fold(changelog: &str, version: &str, section: &str) -> Result<String, String> {
    let lines: Vec<&str> = changelog.split_inclusive('\n').collect();
    let heading = |l: &str| l.trim_end().strip_prefix("## ").map(|h| h.split_whitespace().next().unwrap_or("").to_string());
    if lines.iter().any(|l| heading(l).as_deref() == Some(version)) {
        return Err(format!("{CHANGELOG} already has a section for {version}"));
    }
    let at = lines.iter().position(|l| l.starts_with("## "));
    let mut out = String::new();
    match at {
        Some(i) => {
            out.extend(lines[..i].iter().copied());
            out.push_str(section);
            out.push('\n');
            out.extend(lines[i..].iter().copied());
        }
        None => {
            out.push_str(changelog);
            if !changelog.is_empty() && !changelog.ends_with("\n\n") {
                out.push_str(if changelog.ends_with('\n') { "\n" } else { "\n\n" });
            }
            out.push_str(section);
        }
    }
    Ok(out)
}

fn good_version(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map(|(c, p)| (c, Some(p))).unwrap_or((v, None));
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && pre.is_none_or(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
}

fn good_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub fn usage() -> &'static str {
    "usage: delphi release [status]\n       delphi release changelog [--check | --write <version> [--date <YYYY-MM-DD>]] [--repo <dir>]"
}

/// `delphi release changelog ...` in `repo`: the exit code, what goes to stdout and what goes to stderr.
pub fn run(repo: &Path, check: bool, write: Option<&str>, date: Option<&str>, today: &str) -> (i32, String, String) {
    let files = match listing(repo) {
        Ok(f) => f,
        Err(e) => return (2, String::new(), format!("delphi release changelog: {e}")),
    };
    let bad = problems(&files);
    if !bad.is_empty() {
        let lines: Vec<String> = bad.iter().map(|(n, why)| format!("{DIR}/{n}: {why}")).collect();
        return (1, String::new(), format!("{}\ndelphi release changelog: {} fragment problem(s); nothing was written", lines.join("\n"), bad.len()));
    }
    let frags = fragments(&files);
    if check {
        return (0, format!("delphi release changelog --check: {} fragment(s) under {DIR}/, all well formed\n", frags.len()), String::new());
    }
    let Some(version) = write else {
        if frags.is_empty() {
            return (0, format!("no fragments under {DIR}/: the next release has no entries yet\n"), String::new());
        }
        return (0, section("Unreleased", None, &frags), String::new());
    };
    let version = version.strip_prefix('v').unwrap_or(version);
    if !good_version(version) {
        return (2, String::new(), format!("delphi release changelog: `{version}` is not a version (X.Y.Z)\n{}", usage()));
    }
    let date = date.unwrap_or(today);
    if !good_date(date) {
        return (2, String::new(), format!("delphi release changelog: `{date}` is not a date (YYYY-MM-DD)"));
    }
    if frags.is_empty() {
        return (1, String::new(), format!("delphi release changelog: no fragments under {DIR}/, so there is nothing to write; write the section by hand"));
    }
    let path = repo.join(CHANGELOG);
    let old = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return (2, String::new(), format!("delphi release changelog: cannot read {CHANGELOG}: {e}")),
    };
    let new = match fold(&old, version, &section(version, Some(date), &frags)) {
        Ok(t) => t,
        Err(e) => return (1, String::new(), format!("delphi release changelog: {e}; nothing was written")),
    };
    let tmp: PathBuf = repo.join(format!(".{CHANGELOG}.new"));
    if let Err(e) = fs::write(&tmp, &new).and_then(|_| fs::rename(&tmp, &path)) {
        let _ = fs::remove_file(&tmp);
        return (2, String::new(), format!("delphi release changelog: cannot write {CHANGELOG}: {e}"));
    }
    let mut failed = Vec::new();
    for f in &frags {
        if let Err(e) = fs::remove_file(repo.join(DIR).join(&f.name)) {
            failed.push(format!("{DIR}/{}: {e}", f.name));
        }
    }
    if !failed.is_empty() {
        return (2, String::new(), format!("delphi release changelog: {CHANGELOG} has the {version} section, but these fragments could not be deleted; delete them by hand: {}", failed.join("; ")));
    }
    (0, format!("delphi release changelog: wrote the {version} section ({} entr{}) to {CHANGELOG} and deleted the fragments; add the release's summary above its first subsection\n", frags.len(), if frags.len() == 1 { "y" } else { "ies" }), String::new())
}

/// `delphi release changelog`; `args` start after `changelog`.
pub fn cli(args: &[String]) -> i32 {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let known = ["--check", "--write", "--date", "--repo"];
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if matches!(a.as_str(), "--write" | "--date" | "--repo") {
            skip = true;
        } else if !known.contains(&a.as_str()) {
            eprintln!("delphi release changelog: unexpected `{a}`\n{}", usage());
            return 2;
        }
    }
    let check = args.iter().any(|a| a == "--check");
    let write = flag("--write");
    if (check && write.is_some()) || (args.iter().any(|a| a == "--write") && write.is_none()) || (flag("--date").is_some() && write.is_none()) {
        eprintln!("{}", usage());
        return 2;
    }
    let repo = flag("--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let today = delphi_core::time::now_rfc3339()[..10].to_string();
    let (code, out, err) = run(&repo, check, write.as_deref(), flag("--date").as_deref(), &today);
    print!("{out}");
    if !err.is_empty() {
        eprintln!("{err}");
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(section: &str, body: &str) -> String {
        format!("---\nsection: {section}\n---\n{body}\n")
    }

    #[test]
    fn a_well_formed_fragment_is_read_and_each_broken_rule_is_named() {
        let f = parse("spec-036-notes.md", &frag("Added", "- **Notes from fragments.** One file per change.")).unwrap();
        assert_eq!((f.section, f.body.as_str()), ("Added", "- **Notes from fragments.** One file per change."));
        assert!(parse("crlf.md", "---\r\nsection: Fixed\r\n---\r\n- a\r\n  b\r\n").is_ok_and(|f| f.body == "- a\n  b"));
        let cases: [(&str, String, &str); 10] = [
            ("Upper.md", frag("Added", "- a"), "lowercase slug"),
            ("x.txt", frag("Added", "- a"), "lowercase slug"),
            ("x.md", "- a\n".into(), "starts with a frontmatter"),
            ("x.md", "---\nsection: Added\n- a\n".into(), "does not end"),
            ("x.md", frag("Improved", "- a"), "is not one of"),
            ("x.md", "---\nsection: Added\nspec: 036\n---\n- a\n".into(), "holds one line"),
            ("x.md", "---\n---\n- a\n".into(), "no `section:` line"),
            ("x.md", frag("Fixed", ""), "no entry"),
            ("x.md", frag("Fixed", "A paragraph."), "list item"),
            ("x.md", frag("Fixed", "- a\n### Sub"), "no headings"),
        ];
        for (name, text, want) in cases {
            let e = parse(name, &text).unwrap_err();
            assert!(e.contains(want), "{name} {text:?}: {e}");
        }
        assert!(parse("x.md", &frag("Fixed", "- see [the guide](docs/guides/x.md)")).unwrap_err().contains("would break"));
        assert!(parse("x.md", &frag("Fixed", "- see [the release](https://github.com/o/r/releases)")).is_ok());
        assert!(parse("x.md", "---\nsection: Added\nsection: Fixed\n---\n- a\n").unwrap_err().contains("twice"));
    }

    #[test]
    fn the_readme_is_not_a_fragment_and_a_directory_is_a_problem() {
        let files = vec![(README.to_string(), Some("# How to write one\n".to_string())), ("sub".to_string(), None), ("ok.md".to_string(), Some(frag("Added", "- a")))];
        let p = problems(&files);
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].0, "sub");
        assert_eq!(fragments(&files).len(), 1);
    }

    #[test]
    fn a_section_lists_entries_by_section_order_then_by_file_name() {
        let f = |name: &str, section: &'static str, body: &str| Fragment { name: name.into(), section, body: body.into() };
        let frags = vec![f("b.md", "Fixed", "- fix b"), f("z.md", "Added", "- add z"), f("a.md", "Fixed", "- fix a\n  more"), f("m.md", "Security", "- sec")];
        let s = section("0.7.0", Some("2026-11-25"), &frags);
        assert_eq!(s, "## 0.7.0 — 2026-11-25\n\n### Added\n\n- add z\n\n### Fixed\n\n- fix a\n  more\n- fix b\n\n### Security\n\n- sec\n");
        assert!(section("Unreleased", None, &frags).starts_with("## Unreleased\n\n### Added"));
    }

    #[test]
    fn folding_puts_the_section_above_the_newest_release_and_refuses_a_repeat() {
        let log = "# Changelog\n\nIntro.\n\n## 0.6.6 — 2026-11-20\n\n- old\n";
        let new = fold(log, "0.6.7", "## 0.6.7 — 2026-11-25\n\n### Fixed\n\n- new\n").unwrap();
        assert_eq!(new, "# Changelog\n\nIntro.\n\n## 0.6.7 — 2026-11-25\n\n### Fixed\n\n- new\n\n## 0.6.6 — 2026-11-20\n\n- old\n");
        assert!(fold(log, "0.6.6", "## 0.6.6\n").unwrap_err().contains("already has"));
        assert!(fold(&new, "0.6.7", "x").is_err());
        assert_eq!(fold("# Changelog\n", "1.0.0", "## 1.0.0\n").unwrap(), "# Changelog\n\n## 1.0.0\n");
    }

    fn repo(fragments: &[(&str, String)]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join(CHANGELOG), "# Changelog\n\n## 0.6.6 — 2026-11-20\n\n- old\n").unwrap();
        fs::create_dir(t.path().join(DIR)).unwrap();
        fs::write(t.path().join(DIR).join(README), "# Fragments\n").unwrap();
        for (n, text) in fragments {
            fs::write(t.path().join(DIR).join(n), text).unwrap();
        }
        t
    }

    #[test]
    fn write_folds_the_fragments_in_deletes_them_and_keeps_the_readme() {
        let t = repo(&[("b.md", frag("Fixed", "- fix")), ("a.md", frag("Added", "- add"))]);
        let (code, out, err) = run(t.path(), false, None, None, "2026-11-25");
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.starts_with("## Unreleased\n\n### Added\n\n- add\n"), "{out}");
        let (code, out, err) = run(t.path(), false, Some("v0.6.7"), None, "2026-11-25");
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("wrote the 0.6.7 section (2 entries)"), "{out}");
        let log = fs::read_to_string(t.path().join(CHANGELOG)).unwrap();
        assert_eq!(log, "# Changelog\n\n## 0.6.7 — 2026-11-25\n\n### Added\n\n- add\n\n### Fixed\n\n- fix\n\n## 0.6.6 — 2026-11-20\n\n- old\n");
        let left: Vec<String> = fs::read_dir(t.path().join(DIR)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert_eq!(left, vec![README.to_string()]);
        let (code, _, err) = run(t.path(), false, Some("0.6.8"), None, "2026-11-25");
        assert_eq!(code, 1, "no fragments left, nothing to write");
        assert!(err.contains("nothing to write"));
    }

    #[test]
    fn a_bad_fragment_a_bad_version_or_a_repeated_release_writes_nothing() {
        let t = repo(&[("good.md", frag("Fixed", "- fix")), ("bad.md", frag("Improved", "- x"))]);
        let before = fs::read_to_string(t.path().join(CHANGELOG)).unwrap();
        let (code, _, err) = run(t.path(), false, Some("0.6.7"), None, "2026-11-25");
        assert_eq!(code, 1);
        assert!(err.contains("changelog.d/bad.md") && err.contains("nothing was written"), "{err}");
        assert_eq!(run(t.path(), true, None, None, "2026-11-25").0, 1, "--check fails the same fragment");
        fs::remove_file(t.path().join(DIR).join("bad.md")).unwrap();
        assert_eq!(run(t.path(), true, None, None, "2026-11-25").0, 0);
        assert_eq!(run(t.path(), false, Some("0.6"), None, "2026-11-25").0, 2);
        assert_eq!(run(t.path(), false, Some("0.6.7"), Some("25/11/2026"), "2026-11-25").0, 2);
        let (code, _, err) = run(t.path(), false, Some("0.6.6"), None, "2026-11-25");
        assert_eq!(code, 1, "{err}");
        assert_eq!(fs::read_to_string(t.path().join(CHANGELOG)).unwrap(), before);
        assert!(t.path().join(DIR).join("good.md").exists(), "a refused write deletes no fragment");
    }

    #[test]
    fn a_repository_without_the_directory_has_no_fragments() {
        let t = tempfile::tempdir().unwrap();
        let (code, out, _) = run(t.path(), false, None, None, "2026-11-25");
        assert_eq!(code, 0);
        assert!(out.contains("no fragments"));
    }

    #[test]
    fn versions_and_dates() {
        for v in ["0.6.7", "1.0.0", "1.2.3-rc.1"] {
            assert!(good_version(v), "{v}");
        }
        for v in ["0.6", "v0.6.7", "0.6.x", "1.2.3-", ""] {
            assert!(!good_version(v), "{v}");
        }
        assert!(good_date("2026-11-25") && !good_date("2026-1-25") && !good_date("2026/11/25"));
    }

    /// The fragments committed in this repository are well formed (the lint says the same; this holds without running it).
    #[test]
    fn the_repos_own_fragments_are_well_formed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = listing(&root).unwrap();
        assert!(files.iter().any(|(n, _)| n == README), "{DIR}/{README} says how to write a fragment");
        assert_eq!(problems(&files), Vec::<(String, String)>::new());
    }
}
