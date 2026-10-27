//! `delphi xenia ...`: the org shared-memory store (specs/011, specs/012).
//!
//! Member commands (this milestone, X9): `whoami`, `join`, `sync`, `status`, and the six that publish through the
//! gate: `publish`, `endorse`, `dispute`, `withdraw`, `restore`, `republish` (and `retract`, which withdraws one's own
//! endorsement). Admin commands (`xenia admin ...`) are milestone X8 and are not here.
//!
//! **Humans sign; agents don't.** Every command that signs shows the approval card (the exact bytes, the flags, the
//! similar records) and then makes the operator type the card's short code on a terminal. With stdin or stdout not a
//! terminal it refuses: there is no flag, environment variable or pipe that answers for the operator. The key is the
//! install's xenia key and it is read only here, by the process the operator started.

use crate::home;
use delphi_core::xenia_action::{self, Action, ScopeCtx};
use delphi_core::xenia_publish::{Approver, Decision, NoIndex, Preview, PublishError, Publisher, SimilarRecords};
use delphi_core::xenia_status::{self, StoreRef};
use delphi_core::xenia_sync::{VerifiedSync, VerifyConfig};
use delphi_xenia::identity::Identity;
use delphi_xenia::pins::JoinRequest;
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Where this install's Xenia key lives (spec 011 §3.3 puts all Xenia state under `$DELPHI_HOME/xenia/`).
pub fn key_path(home: &std::path::Path) -> PathBuf {
    home.join("xenia").join("identity.key")
}

const USAGE: &str = "usage:
  delphi xenia whoami
  delphi xenia join <store-url> --org <org> --store <store> --admin <fingerprint> --head <epoch>:<hash> [--allow-local]
  delphi xenia sync [--store <org>/<store>]
  delphi xenia status [--store <org>/<store>] [--json]
  delphi xenia publish <file> --title <t> --summary <s> --promoted [--tags a,b] [--type finding|decision|gotcha|how-to] [--review-by YYYY-MM-DD]
  delphi xenia endorse <id> [--scope a,b] [--note <n>]       delphi xenia dispute <id> --note <n>
  delphi xenia withdraw <id> --reason <r> [--note <n>]       delphi xenia restore <id> [--note <n>]
  delphi xenia retract <id>                                   delphi xenia republish --old <did>
  delphi xenia help [topic] [--json]
  delphi xenia explain <slug or pasted message> [--json]
    [--workspace <name>] [--home <dir>]";

/// Flags that take no value.
const SWITCHES: [&str; 3] = ["--promoted", "--allow-local", "--json"];

#[derive(Debug)]
struct Parsed {
    positional: Vec<String>,
    opts: BTreeMap<String, String>,
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let (mut positional, mut opts) = (Vec::new(), BTreeMap::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            if SWITCHES.contains(&a.as_str()) {
                opts.insert(name.to_string(), "true".to_string());
            } else {
                let v = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
                opts.insert(name.to_string(), v.clone());
            }
        } else {
            positional.push(a.clone());
        }
    }
    Ok(Parsed { positional, opts })
}

pub fn run(args: &[String]) -> i32 {
    run_with(args, &mut TtyApprover)
}

/// [`run`] with the approver injected, so tests can answer the card. The real one is [`TtyApprover`].
pub(crate) fn run_with(args: &[String], approver: &mut dyn Approver) -> i32 {
    let Some(word) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let parsed = match parse(&args[1..]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi xenia {word}: {e}");
            return 2;
        }
    };
    let home = home::delphi_home(parsed.opts.get("home").map(String::as_str));
    match word {
        "whoami" => whoami(&home),
        "join" => join(&home, &parsed),
        "sync" => sync(&home, &parsed),
        "status" => status(&home, &parsed),
        "help" => help(&parsed),
        "explain" => explain(&parsed),
        w if Action::WORDS.contains(&w) => act(&home, w, &parsed, approver),
        "admin" => {
            eprintln!("delphi xenia admin: not built yet (Phase 1 milestone X8)");
            2
        }
        other => {
            eprintln!("delphi xenia {other}: not a command\n{USAGE}");
            2
        }
    }
}

/// The clock the commands judge freshness by. Tests pin it (a fixture's statement is only fresh around its own date).
#[cfg(test)]
pub(crate) static TEST_NOW: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

fn now_secs() -> i64 {
    #[cfg(test)]
    {
        let t = TEST_NOW.load(std::sync::atomic::Ordering::Relaxed);
        if t != 0 {
            return t;
        }
    }
    delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)
}

/// The workspace this command was run for, if there is one. `status` and `sync` work without it (they just do not
/// touch the search index); `publish` needs it to know which zone a file lives in.
fn quiet_workspace(home: &Path, p: &Parsed) -> Option<home::Workspace> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = match home::resolve_workspace(home, &cwd, p.opts.get("workspace").cloned()) {
        home::Resolve::Found(n) => n,
        home::Resolve::NotInGitRepo if home.join("workspaces").join(crate::start::DEFAULT_WORKSPACE).join("config.toml").exists() => crate::start::DEFAULT_WORKSPACE.to_string(),
        _ => return None,
    };
    Some(home::Workspace { dir: home.join("workspaces").join(&name), name })
}

fn open_kb(ws: &Option<home::Workspace>) -> Option<delphi_engine::kb::Kb> {
    let db = ws.as_ref()?.dir.join("local/state.db");
    db.is_file().then(|| delphi_engine::kb::Kb::open(&db).ok()).flatten()
}

fn pick(home: &Path, p: &Parsed) -> Result<(StoreRef, delphi_core::xenia_store::Store), String> {
    let r = xenia_status::pick_store(home, p.opts.get("store").map(String::as_str))?;
    let s = xenia_status::open(home, &r).map_err(|e| e.to_string())?;
    Ok((r, s))
}

/// Prints this install's DID and its fingerprint, the two things a joiner sends an admin and an admin reads
/// back to a joiner (spec 011 §3.3). Creates the key on first use.
fn whoami(home: &std::path::Path) -> i32 {
    match Identity::load_or_create(&key_path(home)) {
        Ok(id) => {
            println!("did:         {}", id.did());
            println!("fingerprint: {}", id.fingerprint());
            0
        }
        Err(e) => {
            eprintln!("delphi xenia whoami: {e}");
            1
        }
    }
}

/// `join` (spec 011 §3.3): the operator names the store, the admin's fingerprint (read to them over a channel that
/// is not the store) and the roster head pair; the roster the store serves must match both, or nothing is written.
fn join(home: &Path, p: &Parsed) -> i32 {
    let (Some(url), Some(org), Some(store), Some(admin), Some(head)) = (p.positional.first(), p.opts.get("org"), p.opts.get("store"), p.opts.get("admin"), p.opts.get("head")) else {
        eprintln!("delphi xenia join needs a store url, --org, --store, --admin and --head\n{USAGE}");
        return 2;
    };
    let (epoch, hash) = match JoinRequest::parse_head(head) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("delphi xenia join: {}", e.reason());
            return 2;
        }
    };
    let local = p.opts.contains_key("allow-local");
    let st = match delphi_core::xenia_store::Store::open(home, org, store, url, local) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("delphi xenia join: {e}{}", if local { "" } else { " (a local path needs --allow-local)" });
            return 1;
        }
    };
    match st.join(&JoinRequest { url: url.clone(), admin_fingerprint: admin.clone(), head_epoch: epoch, head_hash: hash }) {
        Ok(pins) => {
            let (e, _) = pins.head();
            println!("joined {}/{}: the roster chain verifies to epoch {e} and admin {} is active in it. `delphi xenia sync` fetches and verifies the records.", pins.org(), pins.store(), &admin[..admin.len().min(12)]);
            0
        }
        Err(e) => {
            eprintln!("delphi xenia join: refused ({e}); nothing was written");
            1
        }
    }
}

fn sync(home: &Path, p: &Parsed) -> i32 {
    let (r, store) = match pick(home, p) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("delphi xenia sync: {e}");
            return 2;
        }
    };
    let cfg = VerifyConfig::at(now_secs());
    let outcome = match xenia_status::sync_and_record(&store, &cfg) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("delphi xenia sync: {e}");
            return 1;
        }
    };
    let ws = quiet_workspace(home, p);
    let indexed = match open_kb(&ws) {
        Some(mut kb) => match delphi_engine::xenia_read::index_sync(&store, &mut kb, &cfg, &outcome, &delphi_core::time::now_rfc3339()) {
            Ok(delphi_engine::xenia_read::IndexOutcome::Indexed { in_force, held_back, .. }) => format!("; search index updated ({in_force} in force, {held_back} held back)"),
            Ok(delphi_engine::xenia_read::IndexOutcome::Skipped(why)) => format!("; search index left as it was ({why})"),
            Err(e) => format!("; search index not updated ({e})"),
        },
        None => "; no search index to update (run it from a workspace with a knowledge base)".to_string(),
    };
    let (slug, _, detail) = xenia_status::sync_entry(&outcome);
    match &outcome {
        VerifiedSync::Accepted { head, added, superseded, unchanged, quarantined, .. } => {
            println!("{}: accepted {}: {added} new, {superseded} replaced, {unchanged} unchanged, {} quarantined{indexed}", r.id(), &head[..head.len().min(12)], quarantined.len());
            0
        }
        VerifiedSync::UpToDate { head, .. } => {
            println!("{}: up to date at {}{indexed}", r.id(), &head[..head.len().min(12)]);
            0
        }
        VerifiedSync::Unavailable(_) => {
            println!("{}: the store could not be reached; what you have stays as it was{indexed}", r.id());
            0
        }
        _ => {
            eprintln!("{}: the head was REFUSED ({slug}) and nothing was applied; `delphi xenia status` says why. {}", r.id(), if detail["count"].is_number() { format!("{} path(s) affected.", detail["count"]) } else { String::new() });
            for s in [Some(slug), detail["reason"].as_str()].into_iter().flatten() {
                if let Some(t) = delphi_core::xenia_help::what_to_do(s) {
                    eprintln!("  {t}");
                }
            }
            1
        }
    }
}

fn status(home: &Path, p: &Parsed) -> i32 {
    let stores = match p.opts.get("store") {
        Some(_) => match pick(home, p) {
            Ok((r, _)) => vec![r],
            Err(e) => {
                eprintln!("delphi xenia status: {e}");
                return 2;
            }
        },
        None => xenia_status::list_stores(home),
    };
    if stores.is_empty() {
        eprintln!("delphi xenia status: no store is joined; `delphi xenia join <url> ...`");
        return 2;
    }
    let ws = quiet_workspace(home, p);
    let now = now_secs();
    let cfg = VerifyConfig::at(now);
    let mut worst = 0;
    let mut json = Vec::new();
    for r in &stores {
        let st = match xenia_status::open(home, r).map_err(|e| e.to_string()).and_then(|s| {
            // The clock may have moved a freshness statement since the last sync: bring the zone in line when shown.
            if let Some(mut kb) = open_kb(&ws) {
                let _ = delphi_engine::xenia_read::reindex(&s, &mut kb, &cfg, &delphi_core::time::now_rfc3339());
            }
            xenia_status::status(&s, r, &cfg).map_err(|e| e.to_string())
        }) {
            Ok(st) => st,
            Err(e) => {
                eprintln!("{}: cannot be read: {e}", r.id());
                worst = worst.max(1);
                continue;
            }
        };
        if st.warnings.iter().any(|w| w.severity == xenia_status::Severity::Fail) {
            worst = worst.max(1);
        }
        if p.opts.contains_key("json") {
            json.push(xenia_status::to_json(&st));
        } else {
            println!("{}", xenia_status::render(&st, now));
        }
    }
    if p.opts.contains_key("json") {
        println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
    }
    worst
}

/// `publish`, `endorse`, `dispute`, `retract`, `withdraw`, `restore`, `republish`: build the action, run it through the
/// gate with the card, and journal the outcome.
fn act(home: &Path, word: &str, p: &Parsed, approver: &mut dyn Approver) -> i32 {
    let fail = |m: String| {
        eprintln!("delphi xenia {word}: {m}");
        1
    };
    let (_, store) = match pick(home, p) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let ws = quiet_workspace(home, p);
    let ctx = ScopeCtx { home: home.to_path_buf(), workspace: ws.as_ref().map(|w| w.dir.clone()), today: delphi_core::time::now_rfc3339()[..10].to_string() };
    let action = match Action::build(word, p.positional.first().map(String::as_str), &p.opts, &ctx) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi xenia {word}: {e}");
            return 2;
        }
    };
    let key = match Identity::load(&key_path(home)) {
        Ok(k) => k,
        Err(_) => return fail("this install has no key yet; run `delphi xenia whoami`, send the DID to an admin to be added to the roster, then try again".into()),
    };
    let kb = open_kb(&ws);
    let kb_similar = kb.as_ref().map(delphi_engine::xenia_read::KbSimilar);
    let similar: &dyn SimilarRecords = match &kb_similar {
        Some(k) => k,
        None => &NoIndex,
    };
    let now = now_secs();
    let publisher = Publisher::new(&store, VerifyConfig::at(now), similar);
    let mut card = CardApprover { title: action.title(), inner: approver };
    let result = action.run(&publisher, &key, &mut card);
    xenia_status::record_publish(&store, word, &result, now);
    match result {
        Ok(out) => {
            println!(
                "{}: {} file(s) in commit {}{}. Other installs accept it at their next sync.",
                word,
                out.paths.len(),
                &out.commit[..out.commit.len().min(12)],
                if out.already_there { " (the store already held exactly these bytes)" } else { "" }
            );
            0
        }
        Err(PublishError::Denied) => {
            eprintln!("delphi xenia {word}: cancelled; nothing was signed or sent");
            1
        }
        Err(e) => {
            let code = fail(format!("{e} [{}]; nothing was sent", e.reason()));
            if let Some(t) = delphi_core::xenia_help::what_to_do(e.reason()) {
                eprintln!("  {t}");
            }
            code
        }
    }
}

/// `delphi xenia help [topic]`: fixed text from the catalog, offline, no model. Touches no state.
fn help(p: &Parsed) -> i32 {
    use delphi_core::xenia_help as h;
    let json = p.opts.contains_key("json");
    match p.positional.first() {
        None if json => println!("{}", serde_json::json!({"orientation": h::orientation(), "topics": h::TOPICS.iter().map(|t| serde_json::json!({"name": t.name, "title": t.title})).collect::<Vec<_>>()})),
        None => print!("{}", h::orientation()),
        Some(name) => match h::topic(name) {
            Some(t) if json => println!("{}", serde_json::json!({"topic": t.name, "title": t.title, "body": t.body})),
            Some(t) => println!("{}\n\n{}", t.title, t.body),
            None => {
                eprintln!("delphi xenia help: no topic `{}`\n{}", name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect::<String>(), h::topic_list());
                return 2;
            }
        },
    }
    0
}

/// `delphi xenia explain <slug or pasted message>`. Prints the catalog entry only; the pasted text is never echoed.
fn explain(p: &Parsed) -> i32 {
    use delphi_core::xenia_help as h;
    if p.positional.is_empty() {
        eprintln!("delphi xenia explain needs a slug (as `delphi xenia status` shows it) or the text of a message\n{USAGE}");
        return 2;
    }
    let found = h::find(&p.positional.join(" "));
    if p.opts.contains_key("json") {
        println!("{}", serde_json::to_string_pretty(&found.map(h::entry_json).unwrap_or_else(|| serde_json::json!({"found": false}))).unwrap_or_default());
    } else {
        print!("{}", found.map(h::render_entry).unwrap_or_else(|| h::explain("")));
    }
    if found.is_some() { 0 } else { 1 }
}

// ---------------------------------------------------------------------------------------------------------
// The card, on a terminal

/// Shows the card, then asks. Wraps the real or test approver so the card is printed in one place.
struct CardApprover<'a> {
    title: String,
    inner: &'a mut dyn Approver,
}

impl Approver for CardApprover<'_> {
    fn decide(&mut self, preview: &Preview) -> Decision {
        println!("{}", xenia_action::card(&self.title, preview));
        self.inner.decide(preview)
    }
}

/// The real approver: a terminal on both ends, and the typed code. Nothing else answers: not a flag, not the
/// environment, not a pipe.
pub(crate) struct TtyApprover;

impl Approver for TtyApprover {
    fn decide(&mut self, preview: &Preview) -> Decision {
        let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
        if !on_terminal {
            eprintln!("signing is a human act: run this command in a terminal (it has no non-interactive mode)");
            return Decision::Deny;
        }
        let code = xenia_action::code(preview);
        print!("type the approval code ({code}) to sign, anything else cancels: ");
        let ask = |prompt: &str| -> String {
            use std::io::Write;
            print!("{prompt}");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            line.trim().to_string()
        };
        if ask("") != code {
            return Decision::Deny;
        }
        let ack = if preview.flags.is_empty() { true } else { ask(&format!("{} flag(s) above do not block; type `yes` to sign anyway: ", preview.flags.len())) == "yes" };
        if !ack {
            return Decision::Deny;
        }
        Decision::Approve(preview.approve(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::xenia_fixture::*;
    use delphi_core::xenia_publish::Decision;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    struct Answers {
        yes: bool,
        seen: usize,
    }

    impl Approver for Answers {
        fn decide(&mut self, p: &Preview) -> Decision {
            self.seen += 1;
            if self.yes {
                Decision::Approve(p.approve(true))
            } else {
                Decision::Deny
            }
        }
    }

    #[test]
    fn whoami_creates_the_key_under_xenia_and_is_stable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(whoami(d.path()), 0);
        let first = Identity::load(&key_path(d.path())).unwrap().did();
        assert_eq!(whoami(d.path()), 0);
        assert_eq!(Identity::load(&key_path(d.path())).unwrap().did(), first);
        assert!(d.path().join("xenia/identity.key").is_file());
    }

    #[test]
    fn flags_and_positionals_are_separated_and_a_missing_value_is_an_error() {
        let p = parse(&a(&["file.md", "--title", "T", "--promoted", "--tags", "a,b"])).unwrap();
        assert_eq!(p.positional, vec!["file.md"]);
        assert_eq!((p.opts["title"].as_str(), p.opts["promoted"].as_str(), p.opts["tags"].as_str()), ("T", "true", "a,b"));
        assert!(parse(&a(&["--title"])).unwrap_err().contains("needs a value"));
    }

    #[test]
    fn nothing_runs_without_a_joined_store_and_admin_commands_are_not_here() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path().to_str().unwrap();
        let mut no = Answers { yes: false, seen: 0 };
        assert_eq!(run_with(&a(&["status", "--home", h]), &mut no), 2);
        assert_eq!(run_with(&a(&["sync", "--home", h]), &mut no), 1 + 1);
        assert_eq!(run_with(&a(&["endorse", "X", "--home", h]), &mut no), 1);
        assert_eq!(run_with(&a(&["admin", "add"]), &mut no), 2);
        assert_eq!(run_with(&a(&["bogus"]), &mut no), 2);
        assert_eq!(no.seen, 0);
    }

    /// A store on disk, joined through the command and synced through it.
    fn joined(tmp: &Path) -> (Fx, String) {
        TEST_NOW.store(now(), std::sync::atomic::Ordering::Relaxed);
        let fx0 = Fx::with_cast(tmp);
        let mia = fx0.id("mia");
        let m = delphi_core::xenia_fixture::records::memory_with(&mia, &u(0), 1, "active", "v", "rust", "internal_empirical");
        let c = {
            fx0.put(&m);
            fx0.commit("c1")
        };
        fx0.fresh(&c);
        let home = tmp.join("cli-home");
        std::fs::create_dir_all(&home).unwrap();
        let h = home.to_str().unwrap().to_string();
        let (epoch, hash) = fx0.org.head_pair();
        let mut no = Answers { yes: false, seen: 0 };
        let code = run_with(&a(&["join", &fx0.remote.url(), "--org", "acme", "--store", "acme-main", "--admin", &fx0.org.fingerprint("alice"), "--head", &format!("{epoch}:{hash}"), "--allow-local", "--home", &h]), &mut no);
        assert_eq!(code, 0);
        assert_eq!(run_with(&a(&["sync", "--home", &h]), &mut no), 0);
        (fx0, h)
    }

    #[test]
    fn join_then_sync_then_status_work_through_the_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let (_fx, h) = joined(tmp.path());
        let mut no = Answers { yes: false, seen: 0 };
        assert_eq!(run_with(&a(&["status", "--home", &h, "--json"]), &mut no), 0);
        let home = PathBuf::from(&h);
        let r = xenia_status::list_stores(&home).remove(0);
        let st = xenia_status::status(&xenia_status::open(&home, &r).unwrap(), &r, &VerifyConfig::at(now())).unwrap();
        assert_eq!((st.l1, st.l2), (1, 0));
        assert_eq!(xenia_status::read_journal(&xenia_status::open(&home, &r).unwrap()).len(), 1);
        // A second join of the same store is refused and nothing changes.
        assert_ne!(run_with(&a(&["join", "/nowhere", "--org", "acme", "--store", "acme-main", "--admin", &"a".repeat(64), "--head", "1:blake3:00", "--home", &h]), &mut no), 0);
    }

    #[test]
    fn an_endorsement_goes_through_the_card_and_a_no_sends_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, h) = joined(tmp.path());
        let home = PathBuf::from(&h);
        // The install's key becomes dave's (steward of rust) for this test: write dave's seed as the xenia key.
        let dave = fx.id("dave");
        std::fs::create_dir_all(home.join("xenia")).unwrap();
        dave.store_new(&key_path(&home)).unwrap();
        let before = fx.remote.host_head();
        let mut no = Answers { yes: false, seen: 0 };
        let id = u(0);
        assert_eq!(run_with(&a(&["endorse", &id, "--home", &h]), &mut no), 1);
        assert_eq!(no.seen, 1, "the card was shown and answered no");
        assert_eq!(fx.remote.host_head(), before, "a no sends nothing");
        let mut yes = Answers { yes: true, seen: 0 };
        assert_eq!(run_with(&a(&["endorse", &id, "--home", &h]), &mut yes), 0);
        assert_ne!(fx.remote.host_head(), before);
        // The journal says what happened; a second sync reads the endorsement as L2.
        assert_eq!(run_with(&a(&["sync", "--home", &h]), &mut yes), 0);
        let r = xenia_status::list_stores(&home).remove(0);
        let st = xenia_status::status(&xenia_status::open(&home, &r).unwrap(), &r, &VerifyConfig::at(now())).unwrap();
        assert_eq!((st.l2, st.l1), (1, 0));
        let j = xenia_status::read_journal(&xenia_status::open(&home, &r).unwrap());
        assert!(j.iter().any(|e| e.kind == "publish" && e.outcome == "denied") && j.iter().any(|e| e.kind == "publish" && e.outcome == "published"), "{j:?}");
        // Not a steward of the record's area: refused before any card.
        let erin = fx.id("erin");
        std::fs::remove_file(key_path(&home)).unwrap();
        erin.store_new(&key_path(&home)).unwrap();
        let mut yes2 = Answers { yes: true, seen: 0 };
        assert_eq!(run_with(&a(&["endorse", &id, "--home", &h]), &mut yes2), 1);
        assert_eq!(yes2.seen, 0, "a refusal comes before there is anything to sign");
    }

    #[test]
    fn the_real_approver_refuses_without_a_terminal() {
        // Under the test harness stdin is not a terminal: the real approver must say no, whatever the card says.
        let tmp = tempfile::tempdir().unwrap();
        let fx = Fx::with_cast(tmp.path());
        let c = fx.commit("c1");
        fx.fresh(&c);
        fx.join();
        let _ = fx.sync();
        let mut p = delphi_core::xenia_publish::Preview { items: vec![], flags: vec![], similar: vec![], reminders: vec![], head: "0".repeat(40), digest: "blake3:abc".into() };
        p.digest = "blake3:0123456789abcdef".into();
        if unsafe { libc::isatty(0) == 0 } {
            assert_eq!(TtyApprover.decide(&p), Decision::Deny);
        }
    }
}
