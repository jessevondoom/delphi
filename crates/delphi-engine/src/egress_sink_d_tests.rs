//! The sinks of `specs/141` slice D (AC-4, AC-8): a repository (the daemon's push and `delphi handling check`, both through
//! `worktree::repository_check` over the files `worktree::repo_files` reads) and the row of an outbound send. The outbound card itself
//! is proved through a harness's real request (`harness_run::tests::sink_outbound_card_names_the_marks`); the push through
//! `worktree::tests::a_push_whose_range_holds_a_labelled_page_is_refused_before_the_card_and_after_it`.

use super::*;
use crate::handling::{self, Handling, Trait};
use crate::label::Mark;
use crate::library::{AddOpts, Budgets, Library};
use crate::worktree::{repo_files, repository_check, Pick};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The remote the fixture workspace lists in `[handling] internal_remotes`.
pub(crate) const INTERNAL_REMOTE: &str = "https://git.example.com/team/notes.git";
/// A remote that is not on the list.
pub(crate) const PUBLIC_REMOTE: &str = "https://git.example.com/public/notes.git";

/// A held source's text, long enough that a short quotation fits its aggregate cap.
pub(crate) fn long_text() -> String {
    (0..400).map(|i| format!("Held sentence {i:03} of the tides runs on. ")).collect()
}

/// A page body longer than the 300 characters a copy is found by, its own for each slug (a copy of one page is not a copy of another).
pub(crate) fn page_body(slug: &str) -> String {
    let lines: String = (0..12).map(|i| format!("Line {i:02} of the page {slug} about the harbour and its tides, as the operator wrote it.\n")).collect();
    format!("# Harbour\n\n{lines}")
}

pub(crate) struct Fx {
    _t: tempfile::TempDir,
    pub(crate) home: PathBuf,
    pub(crate) ws: PathBuf,
}

impl Fx {
    pub(crate) fn new() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("config.toml"), format!("[handling]\ninternal_remotes = [\"{INTERNAL_REMOTE}\"]\n")).unwrap();
        Fx { _t: t, home, ws }
    }

    fn lib(&self) -> Library {
        Library::in_home(&self.home).with_workspace(&self.ws)
    }

    /// A library source carrying exactly `mark` (confidential brings internal), with `text` as its kept text. Returns its id.
    pub(crate) fn source(&self, name: &str, text: &str, mark: Mark) -> String {
        let p = self.home.join(name);
        std::fs::write(&p, text).unwrap();
        let opts = if mark == Mark::LibraryPrivate { AddOpts { private: true, licence: Some("own".into()), ..Default::default() } } else { AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() } };
        let id = self.lib().add_file(&p, &opts, &Budgets::default()).unwrap().id;
        let h = Handling::of(&self.ws, &self.home);
        match mark {
            Mark::Personal => {
                h.operator_set(&handling::source_item(&id), Trait::Personal).unwrap();
            }
            Mark::Internal => {
                h.operator_set(&handling::source_item(&id), Trait::Internal).unwrap();
            }
            Mark::Confidential => {
                h.operator_set(&handling::source_item(&id), Trait::Confidential).unwrap();
            }
            Mark::TextHeld => {
                let path = self.lib().source_dir(&id).join("meta.json");
                let mut meta: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                meta["text_held"] = json!(true);
                std::fs::write(&path, meta.to_string()).unwrap();
            }
            Mark::LibraryPrivate => {}
        }
        id
    }

    /// A topic page of the library whose material is `sources`, labelled by the daemon (`delphi handling relabel`). Returns the page
    /// as stored: its frontmatter view, its visible line and its body.
    pub(crate) fn page(&self, slug: &str, sources: &[&str]) -> String {
        let dir = self.lib().pages_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let material: Vec<Value> = sources.iter().map(|s| json!({"id": format!("s-{s}")})).collect();
        std::fs::write(dir.join(format!("{slug}.json")), json!({"kind": "topic_page", "material": material}).to_string()).unwrap();
        std::fs::write(dir.join(format!("{slug}.md")), format!("---\ntitle: {slug}\n---\n\n{}", page_body(slug))).unwrap();
        crate::label::relabel(&self.home, &self.ws, None).unwrap();
        std::fs::read_to_string(dir.join(format!("{slug}.md"))).unwrap()
    }
}

/// A git repository at `dir` (made if absent) with `files` written and staged.
pub(crate) fn staged_repo(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
        git(dir, &["add", rel]);
    }
}

/// Git with no user or system config, as a test's own tool.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What the repository sink did with one output for `remote`: R refuse, L send labelled (refused with nobody to ask, sent once
/// confirmed), Q quotations, S send.
fn outcome(fx: &Fx, root: &Path, files: &[(String, Vec<u8>)], remote: &str) -> char {
    let nobody = repository_check(&fx.home, Some(&fx.ws), root, files, Some(remote), Confirm::Nobody);
    let yes = repository_check(&fx.home, Some(&fx.ws), root, files, Some(remote), Confirm::Terminal(&mut |_| true));
    match (nobody, yes) {
        (Err(_), Err(_)) => 'R',
        (Err(_), Ok(s)) => {
            assert_eq!(s.decision.kind, Kind::SendLabelled);
            'L'
        }
        (Ok(s), Ok(_)) if s.decision.quotations => 'Q',
        (Ok(_), Ok(_)) => 'S',
        (Ok(_), Err(e)) => panic!("a confirmed send refused what an unconfirmed one sent: {e}"),
    }
}

fn want(sink: Sink) -> String {
    Mark::ALL.iter().map(|m| match label::row(sink, *m) { Kind::Refuse => 'R', Kind::SendLabelled => 'L', Kind::Quotations => 'Q', Kind::Send => 'S' }).collect()
}

/// AC-4 and AC-8, the repository row, through `delphi handling check --staged` and `--range`'s reader (`worktree::repo_files`) and the
/// one check both it and the daemon's push run (`worktree::repository_check`). For each mark, a staged copy of a page whose material
/// carries that one mark is decided as the row says, at a remote that is not on `[handling] internal_remotes` and at one that is: an
/// `internal` page passes only to the listed remote, labelled and confirmed. An unlabelled file holding 301 characters of a `text_held`
/// source is refused at both, and a short quotation passes. A refusal names the file and the marks, and no text.
#[test]
fn sink_repository_check_follows_its_row() {
    let fx = Fx::new();
    let (mut public, mut internal) = (String::new(), String::new());
    for m in Mark::ALL {
        let text = if m == Mark::TextHeld { long_text() } else { format!("Source text for the {} fixture.\n", m.as_str()) };
        let id = fx.source(&format!("{}.md", m.as_str()), &text, m);
        let page = fx.page(&format!("harbour-{}", m.as_str()), &[&id]);
        let repo = fx.home.join(format!("repo-{}", m.as_str()));
        staged_repo(&repo, &[("docs/harbour.md", page.as_str()), ("README.md", "# A project\n")]);
        let (root, files) = repo_files(&repo, &Pick::Staged).unwrap();
        assert_eq!(files.len(), 2, "both staged files are read");
        public.push(outcome(&fx, &root, &files, PUBLIC_REMOTE));
        internal.push(outcome(&fx, &root, &files, INTERNAL_REMOTE));
        if let Err(e) = repository_check(&fx.home, Some(&fx.ws), &root, &files, Some(PUBLIC_REMOTE), Confirm::Nobody) {
            assert!(e.contains("docs/harbour.md") && e.contains(m.shown()), "the refusal names the file and the mark: {e}");
            assert!(!e.contains("Line 03 of the page") && !e.contains("README"), "a refusal names no text and no unlabelled file: {e}");
        }
        // no remote named: the strict row (not on the list)
        let none = repository_check(&fx.home, Some(&fx.ws), &root, &files, None, Confirm::Terminal(&mut |_| true));
        assert_eq!(none.is_ok(), matches!(m, Mark::TextHeld), "{m:?}: with no remote an internal file is refused");
    }
    assert_eq!(public, want(Sink::Repository { internal_remote: false }));
    assert_eq!(internal, want(Sink::Repository { internal_remote: true }));
    assert_eq!((public.as_str(), internal.as_str()), ("RRRRQ", "RLRRQ"), "the table of §2.4: internal only to a listed remote");

    // an unlabelled file that holds 301 characters of a held source: refused at both remotes, through a committed range too
    let held = Library::in_home(&fx.home).list().into_iter().find(|(_, m)| m["text_held"] == true).map(|(id, _)| id).unwrap();
    let held_text = std::fs::read_to_string(fx.lib().source_dir(&held).join("source.md")).unwrap();
    // 301 characters that begin and end on a letter, so no whitespace is trimmed from the run
    let chars: Vec<char> = held_text.chars().collect();
    let at = (10..).find(|i| !chars[*i].is_whitespace() && !chars[*i + 300].is_whitespace()).unwrap();
    let quote: String = chars[at..at + 301].iter().collect();
    let repo = fx.home.join("repo-quote");
    staged_repo(&repo, &[("notes.md", "# Notes\n")]);
    git(&repo, &["commit", "--quiet", "-m", "one"]);
    std::fs::write(repo.join("notes.md"), format!("# Notes\n\n{quote}\n")).unwrap();
    git(&repo, &["commit", "--quiet", "-am", "two"]);
    for pick in [Pick::Range("HEAD~1..HEAD"), Pick::Paths(&["notes.md".to_string()])] {
        let (root, files) = repo_files(&repo, &pick).unwrap();
        for remote in [PUBLIC_REMOTE, INTERNAL_REMOTE] {
            let e = repository_check(&fx.home, Some(&fx.ws), &root, &files, Some(remote), Confirm::Terminal(&mut |_| true)).unwrap_err();
            assert!(e.contains("notes.md") && e.contains(&format!("s-{held}")) && !e.contains("Held sentence"), "{e}");
        }
    }
    // a short quotation of it passes
    std::fs::write(repo.join("notes.md"), format!("# Notes\n\n{}\n", held_text.chars().skip(10).take(200).collect::<String>())).unwrap();
    git(&repo, &["commit", "--quiet", "-am", "three"]);
    let (root, files) = repo_files(&repo, &Pick::Range("HEAD~1..HEAD")).unwrap();
    assert!(repository_check(&fx.home, Some(&fx.ws), &root, &files, Some(PUBLIC_REMOTE), Confirm::Nobody).is_ok());
    // the range reader takes no option for a range
    assert!(repo_files(&repo, &Pick::Range("--output=x..HEAD")).is_err() && repo_files(&repo, &Pick::Range("HEAD")).is_err());
}

/// `specs/141` §2.4 item 6: an `internal` file is sent labelled to a listed remote only when it carries its visible line, and only when
/// the operator says yes; what the operator is asked names the marks and the items.
#[test]
fn an_internal_file_goes_to_a_listed_remote_only_with_its_line_and_a_yes() {
    let fx = Fx::new();
    let id = fx.source("int.md", "An internal note.\n", Mark::Internal);
    let page = fx.page("harbour", &[&id]);
    let unlined = crate::label::without_line(&page);
    for (body, line_ok) in [(page.as_str(), true), (unlined.as_str(), false)] {
        let repo = fx.home.join(format!("repo-{line_ok}"));
        staged_repo(&repo, &[("harbour.md", body)]);
        let (root, files) = repo_files(&repo, &Pick::Staged).unwrap();
        let mut asked = String::new();
        let r = repository_check(&fx.home, Some(&fx.ws), &root, &files, Some(INTERNAL_REMOTE), Confirm::Terminal(&mut |q| {
            asked = q.to_string();
            true
        }));
        assert_eq!(r.is_ok(), line_ok, "{r:?}");
        if line_ok {
            assert!(asked.contains("> Handling: INTERNAL") && asked.contains(&handling::source_item(&id)), "{asked}");
            assert!(repository_check(&fx.home, Some(&fx.ws), &root, &files, Some(INTERNAL_REMOTE), Confirm::Terminal(&mut |_| false)).is_err(), "a no keeps it here");
        } else {
            assert!(r.unwrap_err().contains("> Handling: INTERNAL"), "the refusal names the line to add");
        }
    }
}

/// The outbound row (`specs/141` §2.4, last row; slice D): every send is already a card the operator answers, so `personal`,
/// `confidential` and `internal` are sent labelled on that card; `library_private` is held as the private-read mark holds a send;
/// `text_held` is bounded by the one predicate. The card's line names each mark with how many items brought it, and an empty label
/// adds no line. The label is computed from the span's given list and the daemon's records; nothing here sets the private-read mark.
#[test]
fn the_outbound_row_and_its_card_line() {
    assert_eq!(want(Sink::Outbound), "LLLRQ");
    let fx = Fx::new();
    let h = Handling::of(&fx.ws, &fx.home);
    for (i, t) in [("local/summaries/a.md", Trait::Personal), ("local/summaries/b.md", Trait::Personal), ("local/summaries/c.md", Trait::Confidential)] {
        h.operator_set(i, t).unwrap();
    }
    let items: Vec<String> = ["a", "b", "c", "d"].iter().map(|x| format!("local/summaries/{x}.md")).collect();
    handling::note_given(&h, &fx.ws, &handling::span_key("m1", "architect"), &items);
    let given = crate::label::given(&fx.ws, &fx.home, "m1", "architect");
    assert_eq!(given.names(), vec!["personal", "internal", "confidential"]);
    assert_eq!(crate::label::card_line(&given).unwrap(), "Handling: this task was given private (2), internal (1), confidential (1).");
    assert_eq!(check_label(&given, Sink::Outbound).kind, Kind::SendLabelled);
    // another role of the mission was given nothing
    let none = crate::label::given(&fx.ws, &fx.home, "m1", "scout");
    assert!(none.is_empty() && crate::label::card_line(&none).is_none());
    assert_eq!(crate::private_read::state(&fx.ws, &crate::private_read::task_key("m1", "t_0001")), crate::private_read::State::Unmarked);
    // both outbound senders put the line on their cards and ask the row
    for f in ["harness_run.rs", "connector_run.rs"] {
        let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(f)).unwrap();
        let prod = src.split("\n#[cfg(test)]\nmod tests").next().unwrap();
        assert!(prod.contains("label::given(") && prod.contains("label::card_line(") && prod.contains("Sink::Outbound") && prod.contains("handling.refused()"), "{f}");
    }
}
