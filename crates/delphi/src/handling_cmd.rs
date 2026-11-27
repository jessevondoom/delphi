//! `delphi handling` (`specs/121` §3 items 3, 5 and 7): the operator's view of the handling traits (shown as private, internal and
//! confidential), the operator's set and clear, the reconciliation, `relabel` (`specs/141` §2.1 item 4), and the repository sink of
//! `specs/141` §2.4 item 6: `check` (what a commit or a push would send) and `hook` (the pre-push hook that runs it).
//!
//! `set` and `clear` are the operator's alone: both are refused inside an agent seat and without a terminal. `clear` declassifies
//! and is the one caller of `Handling::operator_clear` (a test in `delphi_engine::handling` enumerates the callers); `set` gives an
//! `internal` the research weight of §7, which no agent or host may give.

use std::io::IsTerminal;
use std::path::Path;

use delphi_engine::handling::{self, Handling, Trait};
use delphi_engine::handling_reconcile;

pub const USAGE: &str = "usage: delphi handling status [--json]\n       delphi handling set <item> <private|internal|confidential>\n       delphi handling clear <item> <private|internal|confidential>\n       delphi handling reconcile [--dry-run | --apply] [--json]\n       delphi handling relabel [--json]\n       delphi handling check <path>... | --staged | --range <a>..<b> [--remote <url>]\n       delphi handling hook [--install]\n(each takes [--workspace <name>] [--home <dir>])";

/// The marker of a reconciliation that was applied: until there is one, `reconcile` with no flag is a dry run (§3 item 7).
const APPLIED: &str = "reconciled";

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if matches!(a.as_str(), "--workspace" | "--home" | "--range" | "--remote") {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

/// An item as the operator names it: an id (`local/summaries/x.md`, `library/claims/<id>`), a library pointer
/// (`@library/claim/<id>`), or a path to a file inside the workspace.
pub fn item_named(ws_dir: &Path, name: &str) -> Option<String> {
    let name = name.trim();
    if let Some(i) = handling::item_of("", name) {
        return Some(i);
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return handling::item_in_ws(ws_dir, &std::fs::canonicalize(p).ok()?).or_else(|| handling::item_in_ws(ws_dir, p));
    }
    let item = name.trim_start_matches("./").to_string();
    handling::item_ok(&item).then_some(item)
}

fn refused_unless_operator(cmd: &str) -> Option<i32> {
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi handling {cmd}: refused; handling traits are set and cleared by the operator, and this process was started by an agent seat");
        return Some(2);
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("delphi handling {cmd}: refused; run it yourself at a terminal (this process has none)");
        return Some(2);
    }
    None
}

pub fn run(args: &[String], home: &Path, ws_dir: &Path) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let pos = positional(args);
    match pos.first().copied() {
        Some("status") => match handling_reconcile::status(ws_dir, home) {
            Ok(r) => {
                if json {
                    println!("{}", r.to_json());
                } else {
                    println!("handling (what would be labelled if the rules ran now; nothing was changed):");
                    for l in r.lines().iter().filter(|l| !l.starts_with("run `")).map(|l| l.trim_start_matches("dry run: nothing was changed. ")) {
                        println!("  {l}");
                    }
                    for bad in Handling::of(ws_dir, home).unreadable() {
                        println!("  the record {} cannot be read", bad.display());
                    }
                }
                0
            }
            Err(e) => {
                eprintln!("delphi handling status: {e}");
                1
            }
        },
        Some(cmd @ ("set" | "clear")) => {
            if let Some(code) = refused_unless_operator(cmd) {
                return code;
            }
            let (Some(name), Some(t)) = (pos.get(1), pos.get(2)) else {
                eprintln!("{USAGE}");
                return 2;
            };
            let Some(t) = Trait::parse(t) else {
                eprintln!("delphi handling {cmd}: `{t}` is not a trait (private, internal or confidential)");
                return 2;
            };
            let Some(item) = item_named(ws_dir, name) else {
                eprintln!("delphi handling {cmd}: `{name}` names no item (an id such as local/summaries/x.md or library/claims/<id>, or a file in the workspace)");
                return 2;
            };
            let h = Handling::of(ws_dir, home);
            let done = if cmd == "set" { h.operator_set(&item, t).map(|_| ()) } else { h.operator_clear(&item, t).map(|_| ()) };
            match done {
                Ok(()) => {
                    let now = h.set(&item);
                    println!("{item}: {}", if now.is_empty() { "unlabelled".to_string() } else { handling::shown(&now) });
                    if cmd == "clear" {
                        println!("records derived from it lose the trait at the next `delphi handling reconcile`");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("delphi handling {cmd}: {e}");
                    1
                }
            }
        }
        Some("reconcile") => {
            let marker = handling::dir(ws_dir).join(APPLIED);
            let dry = args.iter().any(|a| a == "--dry-run") || (!args.iter().any(|a| a == "--apply") && !marker.exists());
            match handling_reconcile::reconcile(ws_dir, home, dry) {
                Ok(r) => {
                    if !dry {
                        let _ = std::fs::write(&marker, delphi_core::time::now_rfc3339());
                    }
                    if json {
                        println!("{}", r.to_json());
                    } else {
                        if dry && !args.iter().any(|a| a == "--dry-run") {
                            println!("the first reconciliation is a dry run: read the counts, then run it with --apply");
                        }
                        for l in r.lines() {
                            println!("{l}");
                        }
                    }
                    0
                }
                Err(e) => {
                    eprintln!("delphi handling reconcile: {e}");
                    1
                }
            }
        }
        Some("relabel") => relabel(home, ws_dir, json),
        Some("check") => {
            let dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
            check(args, home, ws_dir, &dir, &mut tty_yes)
        }
        Some("hook") => {
            let dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
            hook(args, &dir)
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

/// `delphi handling relabel` (`specs/141` §2.1 item 4): recompute the label of every page from its recorded material and of every row
/// of the index, from the daemon's records. No model is called. A page from before labels, a stale page and a row with no `marks` read
/// as every mark at every sink until this runs (or the page is rebuilt, or the index synced). It computes; it lowers nothing below
/// what the records say.
fn relabel(home: &Path, ws_dir: &Path, json: bool) -> i32 {
    let db = ws_dir.join("local/state.db");
    let mut kb = if db.is_file() { delphi_engine::kb::Kb::open(&db).ok() } else { None };
    match delphi_engine::label::relabel(home, ws_dir, kb.as_mut()) {
        Ok(r) => {
            if json {
                println!("{}", r.to_json());
            } else {
                println!("pages: {} read, {} relabelled", r.pages, r.pages_changed);
                match kb {
                    Some(_) => println!("index rows: {} read, {} relabelled", r.rows, r.rows_changed),
                    None => println!("index rows: no index yet (it is labelled when it is first built)"),
                }
            }
            0
        }
        Err(e) => {
            eprintln!("delphi handling relabel: {e}");
            1
        }
    }
}

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// `delphi handling check` (`specs/141` §2.4 item 6): the repository sink over what a commit or a push would send, run by the
/// pre-push hook. Each file's label is computed from the daemon's records (a page's from its material, a workspace file's from its
/// handling record, and a copy of a labelled page or of a private source's text by what it carries), never from the file's own
/// frontmatter; held text is bounded by the one predicate. `personal`, `confidential` and `library_private` are refused at every
/// remote; `internal` passes only with `--remote` on `[handling] internal_remotes`, carrying its visible line, and after the operator
/// types `send` at the terminal. Exit 0: may be sent; 1: refused (the files and the marks are named, no text); 2: usage.
fn check(args: &[String], home: &Path, ws_dir: &Path, dir: &Path, ask: &mut dyn FnMut(&str) -> bool) -> i32 {
    use delphi_engine::worktree::{repo_files, repository_check, Pick};
    let paths: Vec<String> = positional(args).into_iter().skip(1).map(String::from).collect();
    let range = value(args, "--range");
    let staged = args.iter().any(|a| a == "--staged");
    let pick = match (staged, range, paths.is_empty()) {
        (true, None, true) => Pick::Staged,
        (false, Some(r), true) => Pick::Range(r),
        (false, None, false) => Pick::Paths(&paths),
        _ => {
            eprintln!("delphi handling check: name files, or --staged, or --range <a>..<b> (one of them)\n{USAGE}");
            return 2;
        }
    };
    if args.iter().any(|a| a == "--remote") && value(args, "--remote").is_none_or(|r| r.starts_with("--")) {
        eprintln!("delphi handling check: --remote takes the remote's URL");
        return 2;
    }
    let (root, files) = match repo_files(dir, &pick) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("delphi handling check: refused: the files could not be read ({e})");
            return 1;
        }
    };
    if files.is_empty() {
        println!("handling check: nothing to send");
        return 0;
    }
    let remote = value(args, "--remote");
    match repository_check(home, Some(ws_dir), &root, &files, remote, delphi_engine::egress::Confirm::Terminal(ask)) {
        Ok(sent) => {
            let line = sent.line();
            println!("handling check: {} file(s) may be sent{}", files.len(), if line.is_empty() { String::new() } else { format!(", labelled: {line}") });
            0
        }
        Err(e) => {
            eprintln!("delphi handling check: {e}");
            1
        }
    }
}

/// The operator's yes to a labelled push, typed at the controlling terminal (`/dev/tty`): a hook's stdin is git's list of refs, so it
/// is not read. Inside an agent seat, or with no terminal, the answer is no and nothing labelled is sent.
fn tty_yes(question: &str) -> bool {
    use std::io::{BufRead, Write};
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("{question}\nthis process was started by an agent seat, which cannot confirm a labelled send: refused");
        return false;
    }
    let Ok(tty) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        eprintln!("{question}\nno terminal to confirm a labelled send on: refused");
        return false;
    };
    let mut out = &tty;
    let _ = write!(out, "{question}\nsend it labelled? type `send` to confirm, anything else stops it: ");
    let _ = out.flush();
    let mut line = String::new();
    std::io::BufReader::new(&tty).read_line(&mut line).is_ok() && line.trim() == "send"
}

/// The words every hook this command writes carries; a hook that holds them is Delphi's.
const HOOK_MARK: &str = "delphi handling check";

/// The pre-push hook (`specs/141` §2.4 items 6 and 10). For each ref pushed it checks the commits the remote does not have yet, at
/// that remote's URL, and refuses the push when the check does. It can only refuse (GOVERNANCE §5 item 11). A deleted ref is not
/// checked; a new branch is checked from the first commit no ref of that remote holds; a `delphi` not on PATH refuses.
pub const PRE_PUSH_HOOK: &str = r#"#!/bin/sh
# delphi handling check: the pre-push label check (specs/141 section 2.4 item 6). It can only refuse a push.
# Written by `delphi handling hook --install`; delete this file to stop it.
url="$2"
command -v delphi >/dev/null 2>&1 || { echo "pre-push: delphi is not on PATH, so the label check cannot run; the push is refused" >&2; exit 1; }
while read -r local_ref local_sha remote_ref remote_sha; do
  case "$local_sha" in *[!0]*) ;; *) continue ;; esac
  case "$remote_sha" in
    *[!0]*) base="$remote_sha" ;;
    *)
      first=$(git rev-list --reverse "$local_sha" --not --remotes="$1" | head -n 1)
      [ -n "$first" ] || continue
      base=$(git rev-parse -q --verify "$first^") || base=$(git hash-object -t tree /dev/null)
      ;;
  esac
  delphi handling check --range "$base..$local_sha" --remote "$url" </dev/null || exit 1
done
exit 0
"#;

fn hook_path(dir: &Path) -> Option<std::path::PathBuf> {
    delphi_engine::worktree::pre_push_hook_path(dir)
}

/// Is Delphi's pre-push hook installed for the repository at `dir`? `None` outside a repository.
pub fn hook_installed(dir: &Path) -> Option<bool> {
    let p = hook_path(dir)?;
    Some(std::fs::read_to_string(&p).is_ok_and(|t| t.contains(HOOK_MARK)))
}

/// The line `delphi connect` prints every time (`specs/141` §2.4 item 10): whether the pre-push label check is installed in the
/// project here, and the one command that installs it. Delphi cannot stop a host that pushes with its own git; the hook can refuse.
pub fn hook_line(dir: &Path) -> String {
    match hook_installed(dir) {
        Some(true) => "pre-push label check: installed in this project (a push that carries a labelled page or held text is refused)".into(),
        Some(false) => "pre-push label check: not installed in this project; a host's own git push is not checked. Install it: delphi handling hook --install".into(),
        None => "pre-push label check: not in a git repository here; in a project, `delphi handling hook --install` installs it".into(),
    }
}

/// `delphi handling hook [--install]`: say whether the pre-push hook is installed here, or install it. It never replaces a hook it did
/// not write, and installing is the operator's: refused inside an agent seat.
fn hook(args: &[String], dir: &Path) -> i32 {
    let Some(path) = hook_path(dir) else {
        eprintln!("delphi handling hook: not inside a git repository");
        return 2;
    };
    if !args.iter().any(|a| a == "--install") {
        println!("{}", hook_line(dir));
        return 0;
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi handling hook --install: refused; the hook is the operator's to install, and this process was started by an agent seat");
        return 2;
    }
    match install_hook(&path) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(e) => {
            eprintln!("delphi handling hook --install: {e}");
            1
        }
    }
}

/// Write Delphi's pre-push hook at `path`, executable. A hook Delphi wrote is brought up to date; any other hook is left as it is.
fn install_hook(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(t) if t == PRE_PUSH_HOOK => return Ok(format!("pre-push label check: already installed ({})", path.display())),
        Ok(t) if t.contains(HOOK_MARK) => {}
        Ok(_) => {
            return Err(format!(
                "{} is another hook, and it is not replaced. Add to it, for each pushed ref: delphi handling check --range \"<remote sha>..<local sha>\" --remote \"$2\" </dev/null || exit 1",
                path.display()
            ))
        }
        Err(_) => {}
    }
    use std::os::unix::fs::PermissionsExt;
    path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(path, PRE_PUSH_HOOK)).and_then(|_| std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))).map_err(|e| e.to_string())?;
    Ok(format!("pre-push label check: installed ({})", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-19's command: `relabel` gives a page from before labels its label from its material, with the view and the visible line; a
    /// second run changes nothing.
    #[test]
    fn relabel_gives_a_page_from_before_labels_its_label_and_a_second_run_changes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        let pages = home.join("library/pages");
        std::fs::create_dir_all(&pages).unwrap();
        // a page built before labels: its sidecar lists a passage of an imported page the operator marked internal, and no label
        std::fs::write(pages.join("tides.md"), "---\ntitle: Tides\n---\n\n# Tides\n\nText.\n").unwrap();
        let side = serde_json::json!({"kind": "topic_page", "title": "Tides", "records": [{"id": "s-kb-0000000000000001", "kind": "passage", "rel_path": "summaries/notes.md", "origin": "summaries (external_untrusted)"}], "material": [{"id": "s-kb-0000000000000001", "hash": "h"}]});
        std::fs::write(pages.join("tides.json"), side.to_string()).unwrap();
        Handling::of(&ws, home).operator_set("local/summaries/notes.md", Trait::Internal).unwrap();
        assert_eq!(relabel(home, &ws, true), 0);
        let side: serde_json::Value = serde_json::from_slice(&std::fs::read(pages.join("tides.json")).unwrap()).unwrap();
        assert_eq!(side["label"]["marks"], serde_json::json!(["internal"]));
        let md = std::fs::read_to_string(pages.join("tides.md")).unwrap();
        assert!(md.contains("handling: [internal]\n") && md.contains("> Handling: INTERNAL\n"), "{md}");
        let r = delphi_engine::label::relabel(home, &ws, None).unwrap();
        assert_eq!((r.pages, r.pages_changed), (1, 0));
    }

    const LISTED: &str = "https://git.example.com/team/notes.git";

    fn git(dir: &Path, args: &[&str]) {
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
    }

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// A home and a workspace whose `[handling] internal_remotes` lists [`LISTED`], with a library page labelled `trait` from its
    /// material (an imported note the operator marked), and the page as stored.
    fn labelled_page(home: &Path, t: Trait) -> (std::path::PathBuf, String) {
        let ws = home.join("workspaces/w");
        std::fs::create_dir_all(ws.join("local/summaries")).unwrap();
        std::fs::write(ws.join("config.toml"), format!("[handling]\ninternal_remotes = [\"{LISTED}\"]\n")).unwrap();
        let pages = home.join("library/pages");
        std::fs::create_dir_all(&pages).unwrap();
        let body: String = (0..10).map(|i| format!("Line {i:02} of the page about the harbour and its tides, as the operator wrote it.\n")).collect();
        std::fs::write(pages.join("harbour.md"), format!("---\ntitle: Harbour\n---\n\n# Harbour\n\n{body}")).unwrap();
        let side = serde_json::json!({"kind": "topic_page", "title": "Harbour", "records": [{"id": "s-kb-0000000000000001", "kind": "passage", "rel_path": "summaries/notes.md", "origin": "summaries (external_untrusted)"}], "material": [{"id": "s-kb-0000000000000001", "hash": "h"}]});
        std::fs::write(pages.join("harbour.json"), side.to_string()).unwrap();
        Handling::of(&ws, home).operator_set("local/summaries/notes.md", t).unwrap();
        delphi_engine::label::relabel(home, &ws, None).unwrap();
        let md = std::fs::read_to_string(pages.join("harbour.md")).unwrap();
        (ws, md)
    }

    /// `specs/141` AC-8, the command: `delphi handling check --staged` exits non-zero for a staged copy of a labelled page and for an
    /// unlabelled file that holds 301 characters of a `text_held` source; an `internal` page passes only with `--remote` on the
    /// configured list (and the operator's yes); a plain file passes.
    #[test]
    fn check_staged_refuses_a_labelled_page_and_held_text_and_passes_internal_only_to_a_listed_remote() {
        let yes = &mut |_: &str| true;
        // a personal page: refused at every remote, even with a yes
        let t = tempfile::tempdir().unwrap();
        let (ws, page) = labelled_page(t.path(), Trait::Personal);
        let repo = t.path().join("repo");
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        git(&repo, &["init", "--quiet"]);
        std::fs::write(repo.join("docs/harbour.md"), &page).unwrap();
        git(&repo, &["add", "docs/harbour.md"]);
        for args in [&["check", "--staged"][..], &["check", "--staged", "--remote", LISTED][..]] {
            assert_eq!(check(&argv(args), t.path(), &ws, &repo, yes), 1, "{args:?}");
        }
        // an internal page: only to the listed remote, and only on a yes
        let t = tempfile::tempdir().unwrap();
        let (ws, page) = labelled_page(t.path(), Trait::Internal);
        let repo = t.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "--quiet"]);
        std::fs::write(repo.join("harbour.md"), &page).unwrap();
        git(&repo, &["add", "harbour.md"]);
        assert_eq!(check(&argv(&["check", "--staged"]), t.path(), &ws, &repo, yes), 1, "no remote named");
        assert_eq!(check(&argv(&["check", "--staged", "--remote", "https://git.example.com/public/notes.git"]), t.path(), &ws, &repo, yes), 1, "a remote not on the list");
        assert_eq!(check(&argv(&["check", "--staged", "--remote", LISTED]), t.path(), &ws, &repo, &mut |_| false), 1, "the operator said no");
        let mut asked = String::new();
        assert_eq!(check(&argv(&["check", "--staged", "--remote", LISTED]), t.path(), &ws, &repo, &mut |q| {
            asked = q.to_string();
            true
        }), 0);
        assert!(asked.contains("> Handling: INTERNAL") && asked.contains("harbour.md"), "{asked}");
        // the same file named as a path, committed and checked as a range
        assert_eq!(check(&argv(&["check", "harbour.md", "--remote", LISTED]), t.path(), &ws, &repo, yes), 0);
        git(&repo, &["commit", "--quiet", "-m", "one"]);
        std::fs::write(repo.join("README.md"), "# A project\n").unwrap();
        git(&repo, &["add", "README.md"]);
        git(&repo, &["commit", "--quiet", "-m", "two"]);
        assert_eq!(check(&argv(&["check", "--range", "HEAD~1..HEAD"]), t.path(), &ws, &repo, yes), 0, "the range holds only the plain file");
        // an unlabelled file that holds 301 characters of a held source
        let text: String = (0..300).map(|i| format!("Held sentence {i:03} runs on and on. ")).collect();
        let held = t.path().join("library/sources/held-a");
        std::fs::create_dir_all(&held).unwrap();
        std::fs::write(held.join("meta.json"), serde_json::json!({"type": "webpage", "title": "Held", "licence": "unknown", "capture_level": "full", "text_held": true}).to_string()).unwrap();
        std::fs::write(held.join("source.md"), &text).unwrap();
        std::fs::write(repo.join("notes.md"), format!("# Notes\n\n{}\n", text.chars().skip(5).take(301).collect::<String>())).unwrap();
        git(&repo, &["add", "notes.md"]);
        assert_eq!(check(&argv(&["check", "--staged", "--remote", LISTED]), t.path(), &ws, &repo, yes), 1);
        // usage: one way to name the files
        assert_eq!(check(&argv(&["check", "--staged", "notes.md"]), t.path(), &ws, &repo, yes), 2);
        assert_eq!(check(&argv(&["check"]), t.path(), &ws, &repo, yes), 2);
    }

    /// `specs/141` §2.4 item 10 and AC-17's second half: the line `delphi connect` prints says whether the pre-push hook is installed
    /// and names the one command that installs it; the hook written is executable, can only refuse, and another hook is never replaced.
    #[test]
    fn the_hook_line_says_whether_the_pre_push_check_is_installed_and_install_writes_only_its_own_hook() {
        let t = tempfile::tempdir().unwrap();
        assert!(hook_line(t.path()).contains("not in a git repository") && hook_installed(t.path()).is_none());
        let repo = t.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "--quiet"]);
        let line = hook_line(&repo);
        assert!(line.contains("not installed") && line.contains("delphi handling hook --install"), "{line}");
        let path = hook_path(&repo).unwrap();
        assert!(path.ends_with(".git/hooks/pre-push"), "{}", path.display());
        install_hook(&path).unwrap();
        assert_eq!(hook_installed(&repo), Some(true));
        assert!(hook_line(&repo).contains("installed in this project"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(install_hook(&path).unwrap().contains("already installed"));
        // the hook refuses on the check's non-zero exit and never pushes or skips anything itself
        assert!(PRE_PUSH_HOOK.contains("delphi handling check --range") && PRE_PUSH_HOOK.contains("|| exit 1") && PRE_PUSH_HOOK.contains("--remote \"$url\""));
        assert!(!PRE_PUSH_HOOK.contains("push") || PRE_PUSH_HOOK.lines().filter(|l| !l.starts_with('#')).all(|l| !l.contains("git push") && !l.contains("--no-verify")));
        // another hook is left as it is
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(install_hook(&path).unwrap_err().contains("not replaced"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#!/bin/sh\nexit 0\n");
        assert_eq!(hook_installed(&repo), Some(false));
        // a hooks directory set in the repository's own config is where it goes
        git(&repo, &["config", "core.hooksPath", ".githooks"]);
        assert!(hook_path(&repo).unwrap().ends_with(".githooks/pre-push"));
    }
}
