//! `delphi test review|authorize|refuse|list|revoke`: the operator's side of an authorized test (`specs/092` TA-0r, TA-1).
//!
//! Slice S1. Nothing here runs a command or turns a record into an approval; `authorize` writes a record into the protected
//! store (`delphi_engine::authorization`) and S2 is the slice that may ever read one back to decide a card.
//!
//! * `review` is read-only. It shows, in this order: the manifest as Delphi renders it from structured fields; a dry run
//!   against recent recorded cards; a diff against the operator's earlier grants; who asked, the taint and the span; and last,
//!   collapsed, the agent's own words, stripped of control and escape characters.
//! * `authorize` works at a local terminal only (no pipe, no relay, no agent seat: [`Terminal::interactive`]), refuses when no
//!   second factor is configured, and needs the authenticator code. A request from a host is authorized only by retyping each
//!   literal argv with nothing offered to copy; a request from an agent span below `internal_empirical` is retyped or confirmed.
//! * `revoke` is irreversible and writes an audit event; `refuse` closes a request and keeps its manifest from being re-staged
//!   for 24 hours.

use crate::xenia_terminal::Terminal;
use delphi_engine::authorization as auth;
use serde_json::{json, Value};
use std::path::Path;

/// What the commands need from outside: the clock, and the second factor. Tests give their own.
pub struct Deps<'a> {
    pub now_unix: i64,
    /// The TOTP secret of the configured second factor, or why there is none.
    pub factor: &'a dyn Fn(&Path) -> Result<Vec<u8>, String>,
}

/// The second factor: `[relay] step_up = "totp"` with a `totp_credential`, in the **ratified** configuration (`GOVERNANCE.md` §5.10).
pub fn configured_factor(ws_dir: &Path) -> Result<Vec<u8>, String> {
    const HOW: &str = "run `delphi relay totp-enroll`, put the key in your authenticator, add `step_up = \"totp\"` and `totp_credential = \"keychain:...\"` under [relay] in config.toml, /ratify it, and check it with `delphi relay totp-check <code>`";
    let live = std::fs::read_to_string(ws_dir.join("config.toml")).map_err(|e| format!("cannot read config.toml ({e}); {HOW}"))?;
    let (text, _) = delphi_engine::boot::effective_config(ws_dir, &live).map_err(|e| format!("{e}; {HOW}"))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let relay = v.get("relay");
    let on = relay.and_then(|r| r.get("step_up")).and_then(|s| s.as_str()) == Some("totp");
    let cred = relay.and_then(|r| r.get("totp_credential")).and_then(|s| s.as_str());
    match (on, cred) {
        (true, Some(c)) => delphi_engine::service::totp_secret(c).map_err(|e| format!("the second factor is configured but not usable ({e}); {HOW}")),
        _ => Err(format!("no second factor is configured, so nothing can be authorized (there is no fallback to no step-up): {HOW}")),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter().enumerate().filter(|(i, a)| *a == name && *i + 1 < args.len()).map(|(i, _)| args[i + 1].clone()).collect()
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn fail(msg: impl std::fmt::Display) -> i32 {
    eprintln!("delphi test: {msg}");
    2
}

/// `2h`, `90m`, `45s` or a number of seconds.
pub fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, mul) = match s.chars().last()? {
        'h' => (&s[..s.len() - 1], 3600),
        'm' => (&s[..s.len() - 1], 60),
        's' => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };
    num.parse::<i64>().ok().filter(|n| *n > 0).map(|n| n * mul)
}

// ------------------------------------------------------------------------------------------------ reading the logs

/// What the logs hold: the staged requests (all missions, host sessions included) and some recent approval cards.
pub struct Logs {
    pub staged: Vec<auth::Staged>,
    pub cards: Vec<Value>,
    pub operator: Vec<delphi_core::event::EventEnvelope>,
}

pub fn read_logs(home: &Path, ws_dir: &Path) -> Logs {
    let mut staged = vec![];
    let mut cards = vec![];
    for row in crate::missions::list(ws_dir) {
        let Ok(events) = delphi_core::log::replay(&delphi_core::sessions::dir(ws_dir, &row.id)) else { continue };
        for e in &events {
            if let Some(s) = auth::Staged::from_event(&row.id, e) {
                staged.push(s);
            } else if e.kind == "approval.requested" && cards.len() < 400 {
                cards.push(e.payload.clone());
            }
        }
    }
    staged.sort_by(|a, b| b.staged_at.cmp(&a.staged_at).then_with(|| b.id.cmp(&a.id)));
    Logs { staged, cards, operator: auth::read_audit(home) }
}

fn state_of(s: &auth::Staged, resolved: &std::collections::HashSet<String>, now: i64) -> &'static str {
    if resolved.contains(&s.id) {
        "answered"
    } else if delphi_core::time::parse_rfc3339(&s.staged_at).is_some_and(|t| now - t >= auth::PENDING_TTL_SECS) {
        "expired"
    } else {
        "pending"
    }
}

fn find<'a>(logs: &'a Logs, id: &str) -> Result<&'a auth::Staged, String> {
    let matches: Vec<&auth::Staged> = logs.staged.iter().filter(|s| s.id == id || s.id.starts_with(id) && id.len() >= 6).collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no staged request `{}` (see `delphi test review`)", auth::clean_text(id, 40))),
        _ => Err(format!("`{}` matches more than one request; give more of the id", auth::clean_text(id, 40))),
    }
}

// ------------------------------------------------------------------------------------------------ rendering

fn age(staged_at: &str, now: i64) -> String {
    match delphi_core::time::parse_rfc3339(staged_at) {
        Some(t) if now >= t => crate::missions::ago(Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t as u64)), std::time::UNIX_EPOCH + std::time::Duration::from_secs(now as u64)),
        _ => "—".into(),
    }
}

/// The pending requests, one line each.
pub fn render_pending(logs: &Logs, now: i64) -> String {
    let resolved = auth::resolved_ids(&logs.operator);
    let rows: Vec<&auth::Staged> = logs.staged.iter().filter(|s| state_of(s, &resolved, now) == "pending").collect();
    if rows.is_empty() {
        return "no pending authorization requests\n".into();
    }
    let mut out = String::new();
    for s in &rows {
        out.push_str(&format!("{:<14} {:<10} taint {:<19} {} entr{}  from {}  ({})\n", s.id, s.mission.chars().take(10).collect::<String>(), s.taint_tier.name(), s.request.manifest.len(), if s.request.manifest.len() == 1 { "y" } else { "ies" }, s.requester_label, age(&s.staged_at, now)));
    }
    out.push_str(&format!("{} pending. `delphi test review <id>` shows one; nothing is authorized until you run `delphi test authorize --from-request <id> --run <run>`.\n", rows.len()));
    auth::safe_lines(&out)
}

/// The review card for one request, in the order of `specs/092` TA-0r.
pub fn render_review(s: &auth::Staged, logs: &Logs, store: &auth::Store, expand: bool) -> String {
    let mut o = String::new();
    o.push_str(&format!("Request {}  (tier L0, inert; mission {})\n\n", s.id, auth::clean_text(&s.mission, 60)));
    o.push_str("(1) MANIFEST: what the code will enforce, rendered by Delphi from structured fields\n");
    for (i, e) in s.request.manifest.iter().enumerate() {
        o.push_str(&format!("  entry {}: {} -> {}\n", i + 1, e.tool, auth::render_argv(&e.argv)));
        for f in &e.files {
            o.push_str(&format!("           executes (hashed when you authorize): {}\n", auth::render_argv(std::slice::from_ref(f))));
        }
        o.push_str(&format!("           binary {} (path resolved and hashed when you authorize); provenance {}; scratch noexec; fixed scrubbed environment; working directory {}\n", auth::render_argv(&e.argv[..1]), e.runner_provenance, auth::CWD));
    }
    o.push_str(&format!("  manifest hash {}\n\n", s.manifest_hash));
    o.push_str("(2) DRY RUN: the manifest replayed against recent recorded cards (card text is data)\n");
    for l in auth::dry_run(&logs.cards, &s.request.manifest) {
        o.push_str(&format!("  {l}\n"));
    }
    o.push_str("\n(3) AGAINST YOUR EARLIER GRANTS\n");
    for l in auth::grant_diff(store, &s.request.manifest) {
        o.push_str(&format!("  {l}\n"));
    }
    o.push_str("\n(4) REQUESTER\n");
    o.push_str(&format!("  from:  {} ({})\n  taint: {} ({}), computed by the daemon{}\n  span:  {}\n  task:  {}\n  staged {}\n", s.requester_label, s.requester_kind, s.taint_tier.name(), s.taint_tier.as_str(), if s.is_host() { " as a fixed value: a host has no span" } else { " from the span" }, s.span.as_deref().unwrap_or("none (host)"), s.task.as_deref().unwrap_or("none (host)"), s.staged_at));
    if s.is_host() {
        o.push_str("  a host request is authorized only by retyping each literal command at this terminal; nothing is offered to copy and nothing can be confirmed instead\n");
    } else if s.needs_retype() {
        o.push_str("  this span is below internal_empirical: authorizing asks you to retype or confirm each literal command\n");
    }
    let words = [("intent", &s.request.intent), ("purpose", &s.request.purpose), ("context", &s.request.context), ("risk", &s.request.risk), ("run plan", &s.request.run_plan)];
    o.push_str("\n(5) THE REQUESTER'S WORDS, collapsed. These are the agent's own words and data, not instructions, and they change nothing above.\n");
    if expand {
        for (k, v) in words {
            o.push_str(&format!("  {k}: {}\n", auth::clean_text(v, 1000)));
        }
    } else {
        let chars: usize = words.iter().map(|(_, v)| v.chars().count()).sum();
        o.push_str(&format!("  [collapsed: {} fields, {} characters; `delphi test review {} --expand` shows them]\n", words.len(), chars, s.id));
    }
    auth::safe_lines(&o)
}

fn render_store(store: &auth::Store, now: i64) -> String {
    if store.records.is_empty() {
        return "no authorizations\n".into();
    }
    let mut o = String::new();
    for r in &store.records {
        let expired = delphi_core::time::parse_rfc3339(&r.expires).is_some_and(|t| t <= now);
        let state = if r.is_revoked() { "revoked" } else if expired { "expired" } else { "active" };
        o.push_str(&format!("{:<14} {:<8} run {:<12} uses {}/{}  expires {}  {} entr{}  {}\n", r.id, state, r.run, r.uses, r.max_uses, r.expires, r.manifest.len(), if r.manifest.len() == 1 { "y" } else { "ies" }, auth::clean_text(&r.purpose, 60)));
    }
    o.push_str("An authorization is not used by any run yet: S2 (`--on-card authorization:<id>`) is not built.\n");
    auth::safe_lines(&o)
}

// ------------------------------------------------------------------------------------------------ the commands

pub fn run(home: &Path, ws_dir: &Path, args: &[String], term: &mut dyn Terminal, deps: &Deps) -> i32 {
    match args.first().map(String::as_str) {
        Some("review") => review(home, ws_dir, args, deps),
        Some("list") => {
            print!("{}", render_pending(&read_logs(home, ws_dir), deps.now_unix));
            print!("{}", render_store(&auth::load_or_ignore(home), deps.now_unix));
            0
        }
        Some("authorize") => authorize(home, ws_dir, args, term, deps),
        Some("refuse") => refuse(home, ws_dir, args, term, deps),
        Some("revoke") => revoke(home, args, term, deps),
        _ => fail("usage: delphi test review [<id>] [--expand] | authorize --from-request <id> --run <run> | refuse <id> | list | revoke <id> (delphi help test)"),
    }
}

fn review(home: &Path, ws_dir: &Path, args: &[String], deps: &Deps) -> i32 {
    let logs = read_logs(home, ws_dir);
    let Some(id) = args.get(1).filter(|a| !a.starts_with("--")) else {
        print!("{}", render_pending(&logs, deps.now_unix));
        return 0;
    };
    match find(&logs, id) {
        Ok(s) => {
            let store = auth::load_or_ignore(home);
            print!("{}", render_review(s, &logs, &store, has(args, "--expand")));
            0
        }
        Err(e) => fail(e),
    }
}

/// Everything `authorize` must hold before it asks for anything: a person at a terminal and a second factor.
fn gate(ws_dir: &Path, term: &mut dyn Terminal, deps: &Deps) -> Result<Vec<u8>, String> {
    if crate::xenia_terminal::inside_an_agent_seat() {
        return Err("refused: this process was started by an agent seat; an authorization is the operator's act".into());
    }
    term.interactive().map_err(|e| format!("refused: authorizing works at a local terminal only (not over a relay, a pipe or a script): {e}"))?;
    (deps.factor)(ws_dir)
}

fn authorize(home: &Path, ws_dir: &Path, args: &[String], term: &mut dyn Terminal, deps: &Deps) -> i32 {
    let secret = match gate(ws_dir, term, deps) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let Some(run_id) = flag(args, "--run") else { return fail("`--run <id>` is required: the id of the run this authorization is for; the launch must pass the same id") };
    let (expires, uses) = match (flag(args, "--expires").map(|e| parse_duration(&e)), flag(args, "--uses").map(|u| u.parse::<u32>().ok())) {
        (Some(None), _) => return fail("`--expires` takes 2h, 90m, 45s or seconds"),
        (_, Some(None)) => return fail("`--uses` takes a number"),
        (e, u) => (e.flatten().unwrap_or(auth::DEFAULT_EXPIRY_SECS), u.flatten().unwrap_or(auth::DEFAULT_USES)),
    };
    let logs = read_logs(home, ws_dir);
    let store = match auth::load(home) {
        Ok(s) => s,
        Err(e) => return fail(format!("refused: {e}; run `delphi doctor`")),
    };
    // What is being authorized: a staged request, or entries the operator wrote at the console.
    type Source<'a> = (Vec<auth::Entry>, String, Option<(String, String, String)>, Option<&'a auth::Staged>);
    let (mut entries, intent, request, staged): Source = match flag(args, "--from-request") {
        Some(id) => {
            let s = match find(&logs, &id) {
                Ok(s) => s,
                Err(e) => return fail(e),
            };
            if auth::resolved_ids(&logs.operator).contains(&s.id) {
                return fail(format!("{} was already answered; a changed or repeated request is a new request", s.id));
            }
            // The daemon never stages an agent request at federated_peer or lower; a log that says so is not trusted.
            if !s.is_host() && s.taint_tier <= delphi_core::provenance::Tier::L1 {
                return fail(format!("{} claims an agent span at {} taint, which cannot be staged; it is not authorized", s.id, s.taint_tier.name()));
            }
            if state_of(s, &auth::resolved_ids(&logs.operator), deps.now_unix) == "expired" {
                return fail(format!("{} expired after seven days; ask again", s.id));
            }
            print!("{}", render_review(s, &logs, &store, false));
            (s.request.manifest.clone(), s.request.intent.clone(), Some((s.id.clone(), s.request_hash.clone(), s.requester_label.clone())), Some(s))
        }
        None => {
            let raw = flags(args, "--entry-json");
            if raw.is_empty() {
                return fail("give `--from-request <id>` (see `delphi test review`), or write the entries yourself with `--entry-json '{\"tool\":\"shell\",\"argv\":[...],\"files\":[...],\"runner_provenance\":\"project_snapshot\"}'`");
            }
            let mut es = vec![];
            for r in raw {
                match serde_json::from_str::<auth::Entry>(&r) {
                    Ok(e) => es.push(e),
                    Err(e) => return fail(format!("--entry-json: {e}")),
                }
            }
            (es, flag(args, "--intent").unwrap_or_default(), None, None)
        }
    };
    // The operator may narrow the manifest.
    for d in flags(args, "--drop") {
        match d.parse::<usize>() {
            Ok(n) if (1..=entries.len()).contains(&n) => entries[n - 1].tool.clear(),
            _ => return fail(format!("`--drop {}`: no such entry", auth::clean_text(&d, 10))),
        }
    }
    entries.retain(|e| !e.tool.is_empty());
    if entries.is_empty() {
        return fail("every entry was dropped; nothing to authorize (`delphi test refuse <id>` closes the request)");
    }
    // The checks that make an entry admissible run before the operator is asked to retype anything.
    for (i, e) in entries.iter().enumerate() {
        if let Err(e) = auth::check_entry(e) {
            return fail(format!("entry {}: {}", i + 1, e));
        }
    }
    // Retype or confirm the literal argv (spec 092 TA-0r).
    let retype_only = staged.is_some_and(|s| s.is_host());
    let retype_or_confirm = staged.is_some_and(|s| !s.is_host() && s.needs_retype());
    if retype_only || retype_or_confirm {
        for (i, e) in entries.iter().enumerate() {
            let want = auth::render_argv(&e.argv);
            let prompt = if retype_only { format!("entry {}: retype this command exactly (it is not offered again): ", i + 1) } else { format!("entry {}: retype this command exactly, or type CONFIRM to confirm it as shown above: ", i + 1) };
            let typed = term.ask(&prompt);
            let ok = match typed.as_deref().map(str::trim) {
                Some(t) if t == want => true,
                Some("CONFIRM") if retype_or_confirm => true,
                _ => false,
            };
            if !ok {
                return fail(format!("entry {} was not retyped exactly{}; nothing was authorized", i + 1, if retype_only { "" } else { " or confirmed" }));
            }
        }
    } else {
        term.show(&format!("{}\n", entries.iter().enumerate().map(|(i, e)| format!("  entry {}: {}", i + 1, auth::render_argv(&e.argv))).collect::<Vec<_>>().join("\n")));
        if !matches!(term.ask(&format!("authorize {} entr{} for run `{}`, {} use(s), {} seconds? [y/N] ", entries.len(), if entries.len() == 1 { "y" } else { "ies" }, auth::clean_text(&run_id, 64), uses, expires)).as_deref().map(str::trim), Some("y" | "Y" | "yes")) {
            return fail("not confirmed; nothing was authorized");
        }
    }
    let purpose = match flag(args, "--purpose") {
        Some(p) => p,
        None => term.ask("purpose (your words; shown at every use): ").unwrap_or_default(),
    };
    // The record is built (and every binary and file hashed) before the second factor is asked for.
    let now_rfc = delphi_core::time::format_unix_millis(deps.now_unix, 0);
    let rec = match auth::build_record(home, &auth::NewRecord { purpose, intent, run: run_id, expires_in_secs: expires, max_uses: uses, entries, request }, deps.now_unix, &now_rfc) {
        Ok(r) => r,
        Err(e) => return fail(format!("refused: {e}")),
    };
    // Step-up: the authenticator code, checked once per code with wrong guesses rationed (`delphi_core::totp`).
    let Some(code) = term.ask("authenticator code: ") else { return fail("no code given; nothing was authorized") };
    let verifier = delphi_core::totp::Verifier::persistent(ws_dir.join("local/totp-state"));
    if let Err(e) = verifier.check(&secret, code.trim(), deps.now_unix.max(0) as u64) {
        return fail(format!("step-up failed: {e}; nothing was authorized"));
    }
    let mut audit = match auth::open_audit(home) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    if let Err(e) = auth::add_record(home, rec.clone()) {
        return fail(format!("refused: {e}"));
    }
    let payload = json!({"id": rec.id, "request_id": rec.request_id, "request_hash": rec.request_hash, "requester": rec.requester, "manifest_hash": rec.manifest_hash, "run": rec.run, "expires": rec.expires, "max_uses": rec.max_uses, "entries": rec.manifest.len(), "by": "operator", "step_up": "totp", "source": "operator"});
    if let Err(e) = auth::append_audit(&mut audit, auth::CREATED, payload) {
        eprintln!("delphi test: {e}; the record {} was written and is in force", rec.id);
        return 1;
    }
    println!("authorized {}: run `{}`, {} use(s), until {}. Nothing uses it yet: the flag that would is not built (spec 092 S2).", rec.id, rec.run, rec.max_uses, rec.expires);
    0
}

fn refuse(home: &Path, ws_dir: &Path, args: &[String], term: &mut dyn Terminal, deps: &Deps) -> i32 {
    if crate::xenia_terminal::inside_an_agent_seat() {
        return fail("refused: this process was started by an agent seat");
    }
    if let Err(e) = term.interactive() {
        return fail(format!("refused: closing a request works at a local terminal only: {e}"));
    }
    let Some(id) = args.get(1).filter(|a| !a.starts_with("--")) else { return fail("usage: delphi test refuse <id>") };
    let logs = read_logs(home, ws_dir);
    let s = match find(&logs, id) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if auth::resolved_ids(&logs.operator).contains(&s.id) {
        return fail(format!("{} was already answered", s.id));
    }
    let mut audit = match auth::open_audit(home) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let payload = json!({"request_id": s.id, "manifest_hash": s.manifest_hash, "reason": "refused by the operator", "blocks_restage": true, "refused_at": delphi_core::time::format_unix_millis(deps.now_unix, 0), "source": "operator"});
    if let Err(e) = auth::append_audit(&mut audit, auth::REFUSED, payload) {
        return fail(e);
    }
    println!("refused {}: this manifest is not staged again for 24 hours", s.id);
    0
}

fn revoke(home: &Path, args: &[String], term: &mut dyn Terminal, deps: &Deps) -> i32 {
    if crate::xenia_terminal::inside_an_agent_seat() {
        return fail("refused: this process was started by an agent seat; revoking is the operator's act");
    }
    if let Err(e) = term.interactive() {
        return fail(format!("refused: revoking works at a local terminal only: {e}"));
    }
    let Some(id) = args.get(1).filter(|a| !a.starts_with("--")) else { return fail("usage: delphi test revoke <id>") };
    if !matches!(term.ask(&format!("revoke {} for good? A revoked authorization cannot be re-enabled, only replaced by a new one. [y/N] ", auth::clean_text(id, 40))).as_deref().map(str::trim), Some("y" | "Y" | "yes")) {
        return fail("not confirmed; nothing was revoked");
    }
    let now_rfc = delphi_core::time::format_unix_millis(deps.now_unix, 0);
    let mut audit = match auth::open_audit(home) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let rec = match auth::revoke(home, id, &now_rfc) {
        Ok(r) => r,
        Err(e) => return fail(format!("refused: {e}")),
    };
    if let Err(e) = auth::append_audit(&mut audit, auth::REVOKED, json!({"id": rec.id, "manifest_hash": rec.manifest_hash, "run": rec.run, "revoked_at": now_rfc, "by": "operator", "source": "operator"})) {
        eprintln!("delphi test: {e}; {} is revoked all the same", rec.id);
        return 1;
    }
    println!("revoked {}: it takes effect at the next card and can never be re-enabled", rec.id);
    0
}

/// The production entry: the real terminal, clock and second factor.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut term = crate::xenia_terminal::StdTerminal::new(crate::xenia_terminal::inside_an_agent_seat());
    run(home, ws_dir, args, &mut term, &Deps { now_unix: now, factor: &configured_factor })
}

#[cfg(test)]
#[path = "test_cmd_tests.rs"]
mod tests;
