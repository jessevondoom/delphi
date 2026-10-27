//! `delphi xenia admin ...` (spec 011 sections 3.2, 4.1, 8; plan milestone X9a): the genesis ceremony and the roster
//! operations over the admin library, **terminal only**. `add`, `remove` and `rotate` write an unsigned proposal
//! branch; `sign`, `publish`, `refresh` and the genesis steps sign, and only through the terminal approver
//! (`xenia_admin_approve.rs`). Nothing here is reachable from the console socket or the tool registry, and a test pins
//! that: roster signing stays out of the daemon.
//!
//! The CLI process performs each operation itself as the operator's user (the library's `0600` files, its store lock
//! for syncs); it does not talk to the daemon. Outcomes go to the store's journal (`state/journal.jsonl`) as `admin`
//! entries.
//!
//! ```text
//! admin init draft  --org O --store S --founder did=..,person=..,handle=..,role=admin|member,areas=a+b|* ... --out FILE
//! admin init sign   FILE --org O --store S --out SIGFILE        (every founder, on their own machine)
//! admin init create FILE --org O --store S --url URL --sig SIGFILE ...   (one founder; prints the join line)
//! admin add <did> --person P --handle H [--admin] [--steward-areas a,b|*]
//! admin remove <did> [--for-cause]
//! admin rotate <old-did> <new-did> --routine | --compromised         (no default)
//! admin show <n> | sign <n> | publish <n> | refresh
//! ```
//! Exit codes: 0 done, 1 refused or failed with the reason stated, 2 usage.

use crate::home;
use crate::xenia_admin_approve::{self as approve, TermApprover, Terminal};
use delphi_core::xenia_admin::{self, Admin, AdminError, GenesisDraft, ProposalReport};
use delphi_core::xenia_status::{self, StoreRef};
use delphi_core::xenia_store::Store;
use delphi_core::xenia_sync::VerifyConfig;
use delphi_xenia::admin::{NewMember, Operation, ProposalSig, Rotation, DEFAULT_VALIDITY_SECS};
use delphi_xenia::identity::Identity;
use delphi_xenia::store::StoreUrl;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// What a run needs from the outside. Tests build their own; [`run`] builds the real one.
pub struct Env {
    pub home: PathBuf,
    /// Admit `file://` and local-path store URLs. Never true in the real command (a store on a local path is for
    /// tests); there is deliberately no flag or environment variable that turns it on.
    pub allow_file: bool,
    /// Unix seconds.
    pub now: i64,
    pub cwd: PathBuf,
}

pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub term: &'a mut dyn Terminal,
}

pub struct Done {
    pub human: String,
    pub json: Value,
}

pub struct Fail {
    pub code: i32,
    pub reason: String,
    pub msg: String,
}

impl Fail {
    pub fn refused(reason: impl Into<String>, msg: impl Into<String>) -> Fail {
        Fail { code: 1, reason: reason.into(), msg: msg.into() }
    }

    pub fn usage(msg: impl Into<String>) -> Fail {
        Fail { code: 2, reason: "usage".into(), msg: msg.into() }
    }
}

pub type R = Result<Done, Fail>;

// ---------------------------------------------------------------------------------------------------------
// Arguments

/// Words people reach for to skip the question. None of them exists; saying so beats a bare "unknown option".
const NO_SUCH_SHORTCUT: &[&str] = &["--yes", "-y", "--force", "--no-confirm", "--approve", "--assume-yes", "--non-interactive", "--batch"];

pub struct Parsed {
    pub pos: Vec<String>,
    vals: BTreeMap<String, Vec<String>>,
    bools: BTreeSet<String>,
}

impl Parsed {
    pub fn flag(&self, name: &str) -> bool {
        self.bools.contains(name)
    }

    pub fn many(&self, name: &str) -> &[String] {
        self.vals.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn one(&self, name: &str) -> Result<Option<&str>, Fail> {
        match self.many(name) {
            [] => Ok(None),
            [v] => Ok(Some(v.as_str())),
            _ => Err(Fail::usage(format!("{name} was given more than once"))),
        }
    }

    pub fn need(&self, name: &str) -> Result<&str, Fail> {
        self.one(name)?.ok_or_else(|| Fail::usage(format!("{name} is required")))
    }

    /// Comma-separated values of a flag that may repeat, flattened.
    pub fn list(&self, name: &str) -> Vec<String> {
        self.many(name).iter().flat_map(|v| v.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }
}

/// `valued` flags take one value (`--flag v` or `--flag=v`); `bools` take none. Anything else is a usage error.
pub fn parse(args: &[String], valued: &[&str], bools: &[&str]) -> Result<Parsed, Fail> {
    let mut p = Parsed { pos: Vec::new(), vals: BTreeMap::new(), bools: BTreeSet::new() };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == "--" {
            p.pos.extend(args[i..].iter().cloned());
            break;
        }
        if !a.starts_with('-') || a == "-" {
            p.pos.push(a.clone());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        if NO_SUCH_SHORTCUT.contains(&name.as_str()) {
            return Err(Fail::usage(format!("{name}: there is no flag that answers for you; approval is typed on a terminal, by a person, for each signature")));
        }
        if bools.contains(&name.as_str()) {
            if inline.is_some() {
                return Err(Fail::usage(format!("{name} takes no value")));
            }
            p.bools.insert(name);
        } else if valued.contains(&name.as_str()) {
            let v = match inline {
                Some(v) => v,
                None => {
                    let v = args.get(i).ok_or_else(|| Fail::usage(format!("{name} needs a value")))?;
                    i += 1;
                    v.clone()
                }
            };
            p.vals.entry(name).or_default().push(v);
        } else {
            return Err(Fail::usage(format!("unknown option {name}")));
        }
    }
    Ok(p)
}

/// A fresh short code for one prompt (4 characters from an unambiguous alphabet), from the system random source.
fn fresh_code() -> String {
    use std::io::Read;
    const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut b = [0u8; 4];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let h = blake3::hash(format!("{t}:{}", std::process::id()).as_bytes());
        b.copy_from_slice(&h.as_bytes()[..4]);
    }
    b.iter().map(|x| ALPHABET[(*x & 31) as usize] as char).collect()
}

/// An agent seat starts its tools with these set; a signature never comes from inside one.
fn inside_an_agent_seat() -> bool {
    std::env::var_os("DELPHI_BRIDGE_SOCKET").is_some() || std::env::var_os("DELPHI_BRIDGE_TOKEN").is_some()
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The real command: `args` are the words after `admin`.
pub fn run_cli(args: &[String]) -> i32 {
    let env = Env { home: home::delphi_home(None), allow_file: false, now: now_secs(), cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")) };
    let mut term = approve::StdTerminal::new(inside_an_agent_seat(), fresh_code);
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    run_with(args, &env, &mut Io { out: &mut out, err: &mut err, term: &mut term })
}

/// `args` are the words after `admin`. `--json` prints one JSON object on stdout; errors are `{"ok": false, ...}`.
pub fn run_with(args: &[String], env: &Env, io: &mut Io<'_>) -> i32 {
    let json_mode = args.iter().any(|a| a == "--json");
    let result = run(env, io, args);
    match result {
        Ok(d) => {
            if json_mode {
                let mut v = d.json;
                if let Value::Object(m) = &mut v {
                    m.insert("ok".into(), json!(true));
                }
                let _ = writeln!(io.out, "{v}");
            } else {
                let _ = write!(io.out, "{}", d.human);
            }
            0
        }
        Err(f) => {
            if json_mode {
                let _ = writeln!(io.out, "{}", json!({"ok": false, "reason": f.reason, "error": f.msg}));
            } else {
                let _ = writeln!(io.err, "delphi xenia admin: {}", f.msg);
            }
            f.code
        }
    }
}

/// The store an admin command is about: `--store <org>/<store>`, or the only one joined. Org, store and URL come from
/// the pins.
fn open_store(env: &Env, sel: Option<&str>) -> Result<(Store, StoreRef), Fail> {
    let r = xenia_status::pick_store(&env.home, sel).map_err(|m| if m.starts_with("no store is joined") { Fail::refused("not-joined", m) } else { Fail::usage(m) })?;
    let opened = if env.allow_file { Store::open(&env.home, &r.org, &r.store, &r.url, true) } else { xenia_status::open(&env.home, &r) };
    let st = opened.map_err(|e| Fail::refused("store-unusable", format!("{}: {e}", r.id())))?;
    Ok((st, r))
}

fn load_key(env: &Env) -> Result<Identity, Fail> {
    Identity::load(&crate::xenia_cmd::key_path(&env.home)).map_err(|e| Fail::refused("no-key", format!("this install has no usable key ({e}); `delphi xenia whoami` creates one")))
}

fn cfg(env: &Env) -> VerifyConfig {
    VerifyConfig::at(env.now)
}

fn require_human(io: &Io<'_>) -> Result<(), Fail> {
    io.term.interactive().map_err(|m| Fail::refused("not-interactive", m))
}

/// Remove a store directory this call created, and the org and `xenia` directories if that leaves them empty.
fn remove_created(home: &Path, org: &str, store: &str) {
    let org_dir = home.join("xenia").join(org);
    let _ = std::fs::remove_dir_all(org_dir.join(store));
    let _ = std::fs::remove_dir(&org_dir);
    let _ = std::fs::remove_dir(home.join("xenia"));
}

/// Remember what an admin operation did, in the store's journal (slugs and the proposal number; nothing is trusted
/// from it). A journal that cannot be written does not hide the result.
fn journal(env: &Env, store: &Store, outcome: &str, head: Option<String>, detail: Value) {
    let _ = xenia_status::record(store, "admin", outcome, head, detail, env.now);
}

fn run(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    // `admin propose add ...` is `admin add ...`: the operations are the proposals.
    let args: Vec<String> = match args {
        [first, second, rest @ ..] if first == "propose" && matches!(second.as_str(), "add" | "remove" | "rotate") => std::iter::once(second.clone()).chain(rest.iter().cloned()).collect(),
        other => other.to_vec(),
    };
    let args = args.as_slice();
    match args.first().map(String::as_str) {
        Some("init") => init(env, io, &args[1..]),
        Some("add") => add(env, &args[1..]),
        Some("remove") => remove(env, &args[1..]),
        Some("rotate") => rotate(env, &args[1..]),
        Some("show") => show(env, &args[1..]),
        Some("sign") => sign(env, io, &args[1..]),
        Some("publish") => publish(env, io, &args[1..]),
        Some("refresh") => refresh(env, io, &args[1..]),
        _ => Err(Fail::usage("admin takes init, add, remove, rotate, show, sign, publish or refresh")),
    }
}

fn with_home(env: &Env, p: &Parsed) -> Result<Env, Fail> {
    Ok(Env { home: p.one("--home")?.map(PathBuf::from).unwrap_or_else(|| env.home.clone()), allow_file: env.allow_file, now: env.now, cwd: env.cwd.clone() })
}

fn admin_fail(e: AdminError, appr: Option<&TermApprover<'_>>) -> Fail {
    if let (AdminError::Denied, Some(w)) = (&e, appr.and_then(|a| a.why())) {
        return Fail::refused(w.reason(), w.message());
    }
    Fail::refused(e.reason(), e.to_string())
}

fn validity(p: &Parsed) -> Result<i64, Fail> {
    match p.one("--validity-days")? {
        None => Ok(DEFAULT_VALIDITY_SECS),
        Some(d) => d.parse::<i64>().ok().filter(|n| (1..=60).contains(n)).map(|n| n * 86_400).ok_or_else(|| Fail::usage("--validity-days is a whole number of days, 1 to 60")),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| Fail::refused("io", format!("{}: {e} (an existing file is never overwritten)", path.display())))?;
    f.write_all(bytes).map_err(|e| Fail::refused("io", format!("{}: {e}", path.display())))
}

// ---------------------------------------------------------------------------------------------------------
// proposals

fn report_json(r: &ProposalReport) -> Value {
    json!({
        "n": r.n,
        "branch": r.branch,
        "epoch_hash": r.epoch_hash,
        "changes": r.changes.iter().map(|c| json!({"kind": c.kind.slug(), "did": c.did, "person": c.person, "handle": c.handle, "detail": c.detail})).collect::<Vec<_>>(),
        "flags": r.flags.iter().map(|f| f.slug()).collect::<Vec<_>>(),
        "reminders": r.reminders.iter().map(|x| x.slug()).collect::<Vec<_>>(),
        "quorum": {"met": r.quorum.met, "have": r.quorum.have, "need": r.quorum.need, "people": r.quorum.people},
    })
}

fn report_text(r: &ProposalReport) -> String {
    let t = crate::xenia_admin_approve::tame;
    let mut s = format!("proposal {} on branch {}\n  epoch hash {}\n", r.n, r.branch, r.epoch_hash);
    for c in &r.changes {
        s.push_str(&format!("  change: {} {} ({}, {}) {}\n", c.kind.slug(), c.did, t(&c.person), t(&c.handle), t(&c.detail)));
    }
    for f in &r.flags {
        s.push_str(&format!("  flag {}: {}\n", f.slug(), f.text()));
    }
    for x in &r.reminders {
        s.push_str(&format!("  reminder {}: {}\n", x.slug(), x.text()));
    }
    s.push_str(&format!("  signatures: {} of {} people{}\n", r.quorum.have, r.quorum.need, if r.quorum.met { " (quorum met)" } else { "" }));
    s
}

fn propose(env: &Env, store_sel: Option<&str>, op: Operation) -> R {
    let (store, _pins) = open_store(env, store_sel)?;
    let a = Admin::new(&store, cfg(env));
    let prop = a.propose(op).map_err(|e| admin_fail(e, None))?;
    let n = prop.report.n;
    journal(env, &store, "proposed", Some(prop.commit.clone()), json!({"proposal": n, "flags": prop.report.flags.iter().map(|f| f.slug()).collect::<Vec<_>>()}));
    let mut human = report_text(&prop.report);
    human.push_str(&format!("nothing is signed yet: each admin runs `delphi xenia admin sign {n}`, then one runs `delphi xenia admin publish {n}`\n"));
    Ok(Done { human, json: json!({"proposal": report_json(&prop.report), "commit": prop.commit}) })
}

fn add(env: &Env, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store", "--person", "--handle", "--steward-areas"], &["--json", "--admin"])?;
    let [did] = p.pos.as_slice() else { return Err(Fail::usage("admin add takes exactly one <did> (the DID `delphi xenia whoami` prints on the new member's machine)")) };
    let member = NewMember { did: did.clone(), person: p.need("--person")?.to_string(), handle: p.need("--handle")?.to_string(), admin: p.flag("--admin"), steward_areas: p.list("--steward-areas") };
    propose(&with_home(env, &p)?, p.one("--store")?, Operation::Add(member))
}

fn remove(env: &Env, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store"], &["--json", "--for-cause"])?;
    let [did] = p.pos.as_slice() else { return Err(Fail::usage("admin remove takes exactly one <did>")) };
    propose(&with_home(env, &p)?, p.one("--store")?, Operation::Remove { did: did.clone(), for_cause: p.flag("--for-cause") })
}

fn rotate(env: &Env, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store"], &["--json", "--routine", "--compromised"])?;
    let [old, new] = p.pos.as_slice() else { return Err(Fail::usage("admin rotate takes <old-did> <new-did>")) };
    let kind = match (p.flag("--routine"), p.flag("--compromised")) {
        (true, false) => Rotation::Routine,
        (false, true) => Rotation::Compromised,
        (true, true) => return Err(Fail::usage("--routine and --compromised are exclusive")),
        (false, false) => return Err(Fail::usage("admin rotate has no default: say --routine (a planned key change; the member then runs `republish --resign`) or --compromised (the old key may be in someone else's hands; its records and withdrawals lose standing)")),
    };
    propose(&with_home(env, &p)?, p.one("--store")?, Operation::Rotate { old_did: old.clone(), new_did: new.clone(), kind })
}

fn number(p: &Parsed, what: &str) -> Result<u64, Fail> {
    let [n] = p.pos.as_slice() else { return Err(Fail::usage(format!("{what} takes the proposal number"))) };
    n.parse::<u64>().ok().filter(|v| *v >= 1 && v.to_string() == *n).ok_or_else(|| Fail::usage("the proposal number is a whole number from 1"))
}

fn show(env: &Env, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store"], &["--json"])?;
    let n = number(&p, "admin show")?;
    let env = &with_home(env, &p)?;
    let (store, _pins) = open_store(env, p.one("--store")?)?;
    let r = Admin::new(&store, cfg(env)).inspect(n).map_err(|e| admin_fail(e, None))?;
    Ok(Done { human: report_text(&r), json: json!({"proposal": report_json(&r)}) })
}

// ---------------------------------------------------------------------------------------------------------
// signing

fn sign(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store"], &["--json"])?;
    let n = number(&p, "admin sign")?;
    let env = &with_home(env, &p)?;
    require_human(io)?;
    let (store, _pins) = open_store(env, p.one("--store")?)?;
    let key = load_key(env)?;
    let mut appr = TermApprover::new(io.term);
    match Admin::new(&store, cfg(env)).sign(&key, n, &mut appr) {
        Ok(d) => {
            journal(env, &store, "signed", Some(d.commit.clone()), json!({"proposal": n}));
            Ok(Done {
            human: if d.already_there { format!("proposal {n}: your signature is already on the branch (commit {})\n", short(&d.commit)) } else { format!("signed proposal {n}: pushed your signature as commit {}\n", short(&d.commit)) },
            json: json!({"proposal": n, "commit": d.commit, "already_there": d.already_there}),
            })
        }
        Err(e) => Err(admin_fail(e, Some(&appr))),
    }
}

fn short(s: &str) -> String {
    s.chars().take(12).collect()
}

fn delivered(d: &xenia_admin::Delivered, what: &str) -> Done {
    Done {
        human: if d.already_there { format!("{what}: the store already holds exactly these bytes (commit {}); nothing was sent\n", short(&d.commit)) } else { format!("{what}: pushed {} file(s) as commit {} on {} ({} attempt(s))\n  {}\n", d.paths.len(), short(&d.commit), short(&d.parent), d.attempts, d.paths.join("\n  ")) },
        json: json!({"commit": d.commit, "parent": d.parent, "paths": d.paths, "attempts": d.attempts, "already_there": d.already_there}),
    }
}

fn publish(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store", "--validity-days"], &["--json"])?;
    let n = number(&p, "admin publish")?;
    let secs = validity(&p)?;
    let env = &with_home(env, &p)?;
    require_human(io)?;
    let (store, _pins) = open_store(env, p.one("--store")?)?;
    let key = load_key(env)?;
    let mut appr = TermApprover::new(io.term);
    match Admin::new(&store, cfg(env)).publish(&key, n, secs, &mut appr) {
        Ok(d) => {
            journal(env, &store, "published", Some(d.commit.clone()), json!({"proposal": n}));
            Ok(delivered(&d, &format!("published epoch {n}")))
        }
        Err(e) => Err(admin_fail(e, Some(&appr))),
    }
}

fn refresh(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    let p = parse(args, &["--home", "--store", "--validity-days"], &["--json"])?;
    if !p.pos.is_empty() {
        return Err(Fail::usage("admin refresh takes no arguments"));
    }
    let secs = validity(&p)?;
    let env = &with_home(env, &p)?;
    require_human(io)?;
    let (store, _pins) = open_store(env, p.one("--store")?)?;
    let key = load_key(env)?;
    let mut appr = TermApprover::new(io.term);
    match Admin::new(&store, cfg(env)).refresh(&key, secs, &mut appr) {
        Ok(d) => {
            journal(env, &store, "refreshed", Some(d.commit.clone()), json!({}));
            Ok(delivered(&d, "refreshed the freshness statement"))
        }
        Err(e) => Err(admin_fail(e, Some(&appr))),
    }
}

// ---------------------------------------------------------------------------------------------------------
// genesis

fn parse_founder(spec: &str) -> Result<NewMember, Fail> {
    let (mut did, mut person, mut handle, mut admin, mut areas) = (None, None, None, false, Vec::new());
    for part in spec.split(',') {
        let (k, v) = part.split_once('=').ok_or_else(|| Fail::usage(format!("--founder: `{part}` is not key=value")))?;
        match k {
            "did" => did = Some(v.to_string()),
            "person" => person = Some(v.to_string()),
            "handle" => handle = Some(v.to_string()),
            "role" => match v {
                "admin" => admin = true,
                "member" => admin = false,
                other => return Err(Fail::usage(format!("--founder: role is admin or member, not `{other}`"))),
            },
            "areas" => areas = v.split('+').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            other => return Err(Fail::usage(format!("--founder: unknown key `{other}`"))),
        }
    }
    match (did, person, handle) {
        (Some(did), Some(person), Some(handle)) => Ok(NewMember { did, person, handle, admin, steward_areas: areas }),
        _ => Err(Fail::usage("--founder needs did=, person= and handle=")),
    }
}

fn init(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    match args.first().map(String::as_str) {
        Some("draft") => init_draft(env, &args[1..]),
        Some("sign") => init_sign(env, io, &args[1..]),
        Some("create") => init_create(env, io, &args[1..]),
        _ => Err(Fail::usage("admin init takes draft, sign or create (the genesis ceremony has three steps)")),
    }
}

/// Step 1: the roster of the founders, as bytes every founder will read before signing. Signs nothing.
fn init_draft(env: &Env, args: &[String]) -> R {
    let p = parse(args, &["--home", "--org", "--store", "--founder", "--out"], &["--json"])?;
    if !p.pos.is_empty() {
        return Err(Fail::usage("admin init draft takes no positional arguments"));
    }
    let founders: Vec<NewMember> = p.many("--founder").iter().map(|s| parse_founder(s)).collect::<Result<_, _>>()?;
    if founders.is_empty() {
        return Err(Fail::usage("at least one --founder is needed"));
    }
    let (org, store, out) = (p.need("--org")?, p.need("--store")?, p.need("--out")?);
    let d = GenesisDraft::build(org, store, &founders, &delphi_xenia::time::format_utc(env.now)).map_err(|e| admin_fail(e, None))?;
    write_new(Path::new(out), d.bytes())?;
    Ok(Done {
        human: format!("wrote the genesis draft of {org}/{store} to {out}\n  hash {}\nsend the file to every other founder and read this hash to them by voice; each runs `delphi xenia admin init sign`\n", d.hash()),
        json: json!({"org": org, "store": store, "draft": out, "hash": d.hash()}),
    })
}

/// Step 2 (each founder): read the draft, see every founder and role, sign it through the terminal approver.
fn init_sign(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    let p = parse(args, &["--home", "--org", "--store", "--out"], &["--json"])?;
    let [file] = p.pos.as_slice() else { return Err(Fail::usage("admin init sign takes the draft file")) };
    let (org, store, out) = (p.need("--org")?, p.need("--store")?, p.need("--out")?);
    let env = &with_home(env, &p)?;
    require_human(io)?;
    let bytes = std::fs::read(file).map_err(|e| Fail::refused("io", format!("{file}: {e}")))?;
    let draft = GenesisDraft::from_bytes(org, store, &bytes).map_err(|e| admin_fail(e, None))?;
    let key = load_key(env)?;
    let prepared = xenia_admin::prepare_genesis_signature(&draft, &key).map_err(|e| admin_fail(e, None))?;
    let mut appr = TermApprover::new(io.term);
    let decision = delphi_core::xenia_admin::AdminApprover::decide(&mut appr, prepared.preview());
    let signed = match decision {
        xenia_admin::AdminDecision::Approve(a) => prepared.sign(&a, &key).map_err(|e| admin_fail(e, Some(&appr)))?,
        xenia_admin::AdminDecision::Deny => return Err(admin_fail(AdminError::Denied, Some(&appr))),
    };
    let sig = signed.proposal_sig();
    write_new(Path::new(out), &sig.to_json())?;
    Ok(Done { human: format!("signed the draft {} as {}; wrote {out}\nsend it to the founder who runs `admin init create`\n", draft.hash(), sig.kid), json: json!({"draft_hash": draft.hash(), "kid": sig.kid, "sig_file": out}) })
}

/// Step 3 (one founder): assemble every founder's signature, sign the first freshness statement, push to the empty
/// store, and join this install to it.
fn init_create(env: &Env, io: &mut Io<'_>, args: &[String]) -> R {
    let p = parse(args, &["--home", "--org", "--store", "--url", "--sig", "--validity-days"], &["--json"])?;
    let [file] = p.pos.as_slice() else { return Err(Fail::usage("admin init create takes the draft file")) };
    let (org, store_id, url) = (p.need("--org")?.to_string(), p.need("--store")?.to_string(), p.need("--url")?.to_string());
    let secs = validity(&p)?;
    let env = &with_home(env, &p)?;
    require_human(io)?;
    let su = StoreUrl::parse(&url, env.allow_file).map_err(|e| Fail::refused("hostile-url", format!("refused the store address: {e}")))?;
    delphi_xenia::pins::check_url(&su.url).map_err(|e| Fail::refused("hostile-url", format!("refused the store address: {}", e.reason())))?;
    let bytes = std::fs::read(file).map_err(|e| Fail::refused("io", format!("{file}: {e}")))?;
    let draft = GenesisDraft::from_bytes(&org, &store_id, &bytes).map_err(|e| admin_fail(e, None))?;
    let mut sigs = Vec::new();
    for f in p.many("--sig") {
        let raw = std::fs::read(f).map_err(|e| Fail::refused("io", format!("{f}: {e}")))?;
        sigs.push(ProposalSig::parse(&raw).map_err(|w| Fail::refused("bad-signature-file", format!("{f}: {w}")))?);
    }
    if sigs.is_empty() {
        return Err(Fail::usage("at least one --sig is needed (every founding admin signs the draft)"));
    }
    let key = load_key(env)?;
    let root = env.home.join("xenia").join(&org).join(&store_id);
    let existed = root.exists();
    let store = delphi_core::xenia_store::Store::open(&env.home, &org, &store_id, &su.url, env.allow_file).map_err(|e| Fail::refused("store-unusable", e.to_string()))?;
    let mut appr = TermApprover::new(io.term);
    let outcome = match Admin::new(&store, cfg(env)).init_store(&draft, &sigs, &key, secs, &mut appr) {
        Ok(o) => o,
        Err(e) => {
            if !existed {
                remove_created(&env.home, &org, &store_id);
            }
            return Err(admin_fail(e, Some(&appr)));
        }
    };
    xenia_admin::join_store(&store, &outcome.join.request()).map_err(|e| admin_fail(e, None))?;
    journal(env, &store, "created", Some(outcome.head.clone()), json!({"founders": sigs.len()}));
    Ok(Done {
        human: format!("created {org}/{store_id} and joined it\n  first head {}\nread this to every later member, out of band (voice or in person):\n  {}\nthen `delphi xenia sync` here accepts the first head\n", short(&outcome.head), outcome.join.command()),
        json: json!({"org": org, "store": store_id, "head": outcome.head, "join": {"url": outcome.join.url, "admin": outcome.join.admin_fingerprint, "head": format!("{}:{}", outcome.join.head_epoch, outcome.join.head_hash), "command": outcome.join.command()}}),
    })
}

#[cfg(test)]
#[path = "xenia_admin_cmd_tests.rs"]
mod tests;
