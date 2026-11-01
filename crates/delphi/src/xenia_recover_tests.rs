//! Tests for the rewrite recovery (`sync --accept-rewritten-head`) and for what the daemon and the tool registry can
//! reach of the admin operations. Every store is a real git repository driven by the real library code; every roster,
//! statement and record is really signed.

use crate::xenia_terminal::{inside_an_agent_seat, StdTerminal, Terminal};
use delphi_core::xenia_fixture::{self as fxt, Fx};
use std::collections::VecDeque;
use std::path::Path;

enum Ans {
    /// Read the prompt and type what it asks for, as a careful person would.
    Honest,
    Text(&'static str),
    /// End of input.
    Eof,
}

struct Term {
    interactive: Result<(), String>,
    script: VecDeque<Ans>,
    /// What happens when the script is used up.
    default: fn() -> Ans,
    shown: String,
    asked: Vec<String>,
    /// Refuse the digest challenge of any preview that shows this text.
    decline_if: Option<String>,
}

impl Term {
    fn honest() -> Term {
        Term { interactive: Ok(()), script: VecDeque::new(), default: || Ans::Honest, shown: String::new(), asked: Vec::new(), decline_if: None }
    }

    fn script(a: Vec<Ans>) -> Term {
        Term { interactive: Ok(()), script: a.into(), default: || Ans::Eof, shown: String::new(), asked: Vec::new(), decline_if: None }
    }

    fn pipe() -> Term {
        Term { interactive: Err("approving is a human act: run this in a terminal (it has no flag, environment variable, file or pipe that answers for you)".into()), ..Term::honest() }
    }
}

/// What a careful person types for a prompt of this library.
fn honest_answer(prompt: &str) -> String {
    if let Some(i) = prompt.find("type `") {
        let rest = &prompt[i + 6..];
        return rest[..rest.find('`').unwrap()].to_string();
    }
    let i = prompt.rfind("type ").unwrap() + 5;
    let rest = &prompt[i..];
    if let Some(j) = rest.find(" (the first 8") {
        return rest[..j].to_string();
    }
    rest[..rest.find("; anything").unwrap()].to_string()
}

impl Terminal for Term {
    fn interactive(&self) -> Result<(), String> {
        self.interactive.clone()
    }

    fn show(&mut self, text: &str) {
        self.shown.push_str(text);
    }

    fn ask(&mut self, prompt: &str) -> Option<String> {
        self.asked.push(prompt.to_string());
        if let (Some(d), true) = (&self.decline_if, prompt.contains("(the first 8")) {
            if self.shown.rsplit("=== ").next().is_some_and(|last| last.contains(d.as_str())) {
                return Some("no".into());
            }
        }
        match self.script.pop_front().unwrap_or_else(self.default) {
            Ans::Honest => Some(honest_answer(prompt)),
            Ans::Text(t) => Some(t.to_string()),
            Ans::Eof => None,
        }
    }

}

struct Rw {
    _tmp: tempfile::TempDir,
    fx: Fx,
    c2: String,
    r2: String,
    tomb: delphi_xenia::testkit_records::SignedRecord,
    sticky: Vec<u8>,
}

/// An install holding a note and a tombstone; the host then rewrites history to drop the tombstone. `signers` sign a
/// statement naming the replaced head (none: no statement at all).
fn rewritten(signers: &[&str]) -> Rw {
    use delphi_xenia::testkit_records::{self as rk, FreshSpec};
    let tmp = tempfile::tempdir().unwrap();
    let org = delphi_core::xenia_fixture::Org::three_admins();
    let alice = org.key("alice");
    let note = rk::memory(&alice, &fxt::u(0), 1, "active", "note");
    let tomb = rk::tombstone(&alice, &fxt::u(1), 1, &fxt::u(0), "withdraw");
    let (fx, c1) = Fx::installed_on(Fx::new(tmp.path(), org), &[&note, &tomb]);
    let c2 = fx.store.accepted_head().unwrap().unwrap();
    let sticky = std::fs::read(fx.store.paths.state.join("sticky.json")).unwrap();
    fx.remote.reset_hard(&c1);
    fx.remote.remove(&tomb.path);
    fx.remote.remove(&tomb.sidecar_path);
    let mut r2 = fx.commit("rewrite");
    if !signers.is_empty() {
        let mut spec = FreshSpec::for_head(&fx.org, &r2, fxt::now() - 600, 7 * fxt::DAY);
        spec.rewrite_of = Some(c2.clone());
        let keys: Vec<_> = signers.iter().map(|n| fx.id(n)).collect();
        let refs: Vec<&_> = keys.iter().collect();
        let (stmt, sigs) = spec.sign(&refs);
        fx.remote.write("roster/freshness.json", &stmt);
        fx.remote.write("roster/freshness.sigs.json", &sigs);
        r2 = fx.commit("statement");
    }
    Rw { _tmp: tmp, fx, c2, r2, tomb, sticky }
}

impl Rw {
    fn recover(&self, term: &mut Term) -> (i32, String, String) {
        let cfg = fxt::cfg();
        let outcome = self.fx.store.sync_verified(&cfg).unwrap();
        let r = delphi_core::xenia_status::StoreRef { org: "acme".into(), store: "acme-main".into(), url: self.fx.remote.url() };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::xenia_recover::recover_with(&r, &self.fx.store, &cfg, &outcome, term, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn unchanged(&self, what: &str) {
        assert_eq!(self.fx.store.accepted_head().unwrap().as_deref(), Some(self.c2.as_str()), "{what}: still at the old head");
        assert_eq!(std::fs::read(self.fx.store.paths.state.join("sticky.json")).unwrap(), self.sticky, "{what}: state untouched");
    }
}

#[test]
fn a_process_started_by_an_agent_seat_is_never_a_person() {
    let seat = StdTerminal::new(true);
    assert!(seat.interactive().unwrap_err().contains("agent seat"));
    assert!(!inside_an_agent_seat() || std::env::var_os("DELPHI_BRIDGE_SOCKET").is_some() || std::env::var_os("DELPHI_BRIDGE_TOKEN").is_some());
    // SAFETY: only this test reads or writes these two variables.
    unsafe { std::env::set_var("DELPHI_BRIDGE_TOKEN", "x") };
    assert!(inside_an_agent_seat());
    unsafe { std::env::remove_var("DELPHI_BRIDGE_TOKEN") };
}

#[test]
fn the_typed_confirmation_shows_only_printable_text_whatever_the_caller_built() {
    // Found by the Flash red-team sweep: `confirm_typed` printed the caller's lines as they came. Today they are
    // hashes, a timestamp and escaped paths, but the gate for the typed acceptance must not depend on that.
    let mut term = Term::script(vec![Ans::Text("accept abc 1")]);
    let lines = "head \u{1b}[2J\u{1b}[Hall clear\nnew \u{202e}head\u{200b} \u{e0041}\n";
    assert!(crate::xenia_terminal::confirm_typed(&mut term, lines, "accept abc 1", "accept it").is_ok());
    for hidden in ['\u{1b}', '\u{202e}', '\u{200b}', '\u{e0041}'] {
        assert!(!term.shown.contains(hidden), "{hidden:?} reached the screen: {:?}", term.shown);
    }
    assert!(term.shown.contains("\\u{001b}[2J") && term.shown.contains("\\u{202e}head") && term.shown.contains("\\u{e0041}"), "{:?}", term.shown);
    assert_eq!(term.shown.lines().count(), 2, "the line breaks the caller wrote are kept");
}

#[test]
fn a_quorum_statement_and_the_typed_commit_and_count_accept_the_rewrite_and_it_is_journaled() {
    let rw = rewritten(&["alice", "bob"]);
    let mut t = Term::honest();
    let (c, out, err) = rw.recover(&mut t);
    assert_eq!(c, 0, "{out}{err}");
    assert!(t.shown.contains(&rw.tomb.path) && t.shown.contains(&rw.tomb.sidecar_path) && t.shown.contains(&rw.c2) && t.shown.contains(&rw.r2), "{}", t.shown);
    assert!(t.asked[0].contains(&format!("accept {} 2", &rw.r2[..12])), "{:?}", t.asked);
    assert_eq!(rw.fx.store.accepted_head().unwrap().as_deref(), Some(rw.r2.as_str()));
    let log = std::fs::read_to_string(rw.fx.store.paths.state.join("rewrite-log.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2, "{log}");
    let journal = delphi_core::xenia_status::read_journal(&rw.fx.store);
    assert!(journal.iter().any(|e| e.kind == "recovery" && e.outcome == "accepted" && e.detail["count"] == 2), "{journal:?}");
}

#[test]
fn without_an_authentic_quorum_statement_the_recovery_explains_and_changes_nothing() {
    for (what, signers) in [("no statement at all", vec![]), ("a statement signed by one of three admins", vec!["alice"])] {
        let rw = rewritten(&signers);
        let mut t = Term::honest();
        let (c, out, err) = rw.recover(&mut t);
        assert_eq!(c, 1, "{what}: {out}{err}");
        assert!(err.contains("rewrite-quorum-missing") && err.contains("Nothing was changed"), "{what}: {err}");
        assert!(t.asked.is_empty(), "{what}: no question was asked");
        rw.unchanged(what);
    }
}

#[test]
fn an_approval_for_another_commit_a_wrong_count_a_yes_or_a_pipe_is_refused() {
    let rw = rewritten(&["alice", "bob"]);
    for (what, ans) in [
        ("another commit", Ans::Text("accept 000000000000 2")),
        ("fewer paths", Ans::Text("accept 111111111111 1")),
        ("yes", Ans::Text("yes")),
        ("the word alone", Ans::Text("accept")),
        ("end of input", Ans::Eof),
    ] {
        let mut t = Term::script(vec![ans]);
        let (c, out, err) = rw.recover(&mut t);
        assert_eq!(c, 1, "{what}: {out}{err}");
        assert!(t.shown.contains(&rw.tomb.path), "{what}: the paths were listed first");
        rw.unchanged(what);
    }
    // The right commit with a count that is not the list's.
    let wrong_count = format!("accept {} 1", &rw.r2[..12]);
    let mut t = Term::script(vec![Ans::Text(Box::leak(wrong_count.into_boxed_str()))]);
    assert_eq!(rw.recover(&mut t).0, 1);
    rw.unchanged("wrong count");
    // No terminal: refused before anything is shown.
    let mut t = Term::pipe();
    let (c, _, err) = rw.recover(&mut t);
    assert_eq!(c, 1);
    assert!(err.contains("human act"), "{err}");
    assert!(t.shown.is_empty() && t.asked.is_empty());
    rw.unchanged("pipe");
    let journal = delphi_core::xenia_status::read_journal(&rw.fx.store);
    assert!(journal.iter().all(|e| e.outcome != "accepted" || e.kind != "recovery"));
}

#[test]
fn a_host_that_went_back_in_time_is_explained_as_not_recoverable() {
    let tmp = tempfile::tempdir().unwrap();
    let org = delphi_core::xenia_fixture::Org::three_admins();
    let note = delphi_xenia::testkit_records::memory(&org.key("alice"), &fxt::u(0), 1, "active", "note");
    let (fx, _c1) = Fx::installed_on(Fx::new(tmp.path(), org), &[&note]);
    let accepted = fx.store.accepted_head().unwrap().unwrap();
    // Back to the first commit, forced: the host now serves an ancestor of what we hold.
    let first = fx.remote.git(&["rev-list", "--max-parents=0", "HEAD"]).trim().to_string();
    fx.remote.reset_hard(&first);
    fx.remote.force_push();
    let cfg = fxt::cfg();
    let outcome = fx.store.sync_verified(&cfg).unwrap();
    assert!(matches!(&outcome, delphi_core::xenia_sync::VerifiedSync::NonFastForward(n) if n.kind == delphi_core::xenia_store::Divergence::Behind), "{outcome:?}");
    let r = delphi_core::xenia_status::StoreRef { org: "acme".into(), store: "acme-main".into(), url: fx.remote.url() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut t = Term::honest();
    let c = crate::xenia_recover::recover_with(&r, &fx.store, &cfg, &outcome, &mut t, &mut out, &mut err);
    assert_eq!(c, 1);
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("OLDER view") && err.contains("cannot be recovered"), "{err}");
    assert!(t.asked.is_empty());
    assert_eq!(fx.store.accepted_head().unwrap().as_deref(), Some(accepted.as_str()));
}

#[test]
fn the_recovery_is_not_reachable_from_the_daemon_or_a_tool() {
    // The engine never names the library's recovery calls (the engine-source test above also pins this), and the CLI word
    // is the only caller.
    let cmd = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/xenia_cmd.rs")).unwrap();
    assert!(cmd.contains("crate::xenia_recover::run("));
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-engine/src");
    for entry in std::fs::read_dir(engine).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".rs") {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            assert!(!code.contains("accept_rewritten_head") && !code.contains("plan_rewrite"), "{:?}", entry.path());
        }
    }
}
#[test]
fn the_daemon_and_the_tool_registry_reach_only_the_limited_admin_operations() {
    // Tools an agent is offered, with and without write scope: nothing about Xenia but the read-only search.
    let reg = delphi_engine::tools::Registry::builtin();
    let mut names: Vec<String> = Vec::new();
    for write in [false, true] {
        for d in reg.defs(write) {
            names.push(d.name.clone());
            if d.name != "xenia_search" && d.name != "xenia_help" {
                assert!(!format!("{} {}", d.name, d.description).to_lowercase().contains("xenia"), "{} mentions Xenia", d.name);
            }
        }
    }
    assert!(names.iter().filter(|n| n.contains("xenia")).all(|n| n == "xenia_search" || n == "xenia_help"), "only the read-only search and the help text");
    for n in &names {
        for banned in ["sign", "publish", "endorse", "admin", "join", "withdraw", "roster", "tombstone", "dispute", "rotate", "propose", "genesis"] {
            assert!(!n.to_lowercase().contains(banned), "tool `{n}` looks like a Xenia write: {banned}");
        }
    }
    // The console socket answers exactly two admin methods; the narrator, relay and host lists have none.
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-engine/src");
    let svc = std::fs::read_to_string(engine.join("service.rs")).unwrap();
    let mut methods = Vec::new();
    for line in svc.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix('"') {
            if let Some((m, after)) = rest.split_once('"') {
                if after.trim_start().starts_with("=>") && m.contains('.') && m.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_') {
                    methods.push(m.to_string());
                }
            }
        }
    }
    let adminish: Vec<&String> = methods.iter().filter(|m| !m.starts_with("host.")).filter(|m| ["admin", "roster", "rotate", "genesis", "propose", "join"].iter().any(|b| m.contains(b))).collect();
    assert_eq!(adminish, vec!["xenia.admin.prepare", "xenia.admin.decide"], "{methods:?}");
    for m in delphi_core::server::NARRATOR_METHODS.iter().chain(delphi_core::server::RELAY_METHODS.iter()).chain(delphi_core::server::HOST_METHODS.iter()) {
        assert!(!m.contains("xenia"), "{m}");
    }
    // Nothing in the engine can start a genesis, a rewrite recovery or a rotation: the identifiers are not there, so
    // the terminal-only set stays terminal-only whatever an RPC asks.
    for entry in std::fs::read_dir(&engine).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let text = text.split("#[cfg(test)]").next().unwrap_or("");
        for banned in ["init_store", "prepare_genesis_signature", "GenesisDraft", "accept_rewritten_head", "plan_rewrite", "rewrite_propose", "rewrite_sign", "rewrite_publish", "rewrite_candidate", "build_rewrite_statement"] {
            assert!(!text.contains(banned), "{name} mentions {banned}");
        }
    }
    // And the daemon's console admin code classifies by the typed diff before it builds a card.
    let core = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../delphi-core/src/xenia_admin_console.rs")).unwrap();
    assert!(core.contains("classify(&base, &cand, admin.policy.min_admins).map_err(ConsoleError::Terminal)?"));
}


#[test]
fn xenia_state_and_the_operator_switch_are_outside_every_zone_an_agent_can_read() {
    use delphi_core::pathguard::{Access, Guard, Zones};
    use delphi_xenia::store::StorePaths;
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let ws = home.join("workspaces/ws");
    let wt = t.path().join("wt");
    for d in [ws.join("local/ratified"), ws.join("team"), ws.join("base"), wt.join("src"), home.join("xenia"), home.join("operator")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(home.join("xenia/identity.key"), "key").unwrap();
    std::fs::write(home.join("operator/xenia.toml"), "[xenia]\nconsole_admin = false\n").unwrap();
    let zone_roots = vec![ws.clone(), ws.join("local"), ws.join("team"), ws.join("base"), home.join("workspaces"), wt.clone()];
    let paths = StorePaths::new(&home, "acme", "acme-main").unwrap();
    assert!(paths.is_outside_zones(&home, &zone_roots));
    std::os::unix::fs::symlink(home.join("xenia/identity.key"), wt.join("src/key-link")).unwrap();
    std::os::unix::fs::symlink(home.join("operator"), wt.join("src/op-link")).unwrap();
    let g = Guard {
        zones: Zones { local: ws.join("local"), team: ws.join("team"), base: ws.join("base"), ratified: ws.join("local/ratified"), worktree: Some(wt.clone()), projects: vec![], bundles: vec![], payload: None },
        role: "builder".into(),
        read_globs: vec!["**".into()],
        write_globs: vec!["**".into()],
        protected: Default::default(),
    };
    let key = home.join("xenia/identity.key").display().to_string();
    let attempts = [
        "xenia/identity.key", "xenia/acme/acme-main/state/pins.toml", "../xenia/identity.key", "@worktree/../../../xenia/identity.key", "@worktree/src/key-link", "@worktree/src/op-link/xenia.toml", &key,
        "operator/xenia.toml", "../operator/xenia.toml", "local/../operator/xenia.toml",
    ];
    for a in attempts {
        for access in [Access::Read, Access::Write] {
            assert!(g.check(a, access).is_err(), "{a} must not resolve ({access:?})");
        }
    }
}
