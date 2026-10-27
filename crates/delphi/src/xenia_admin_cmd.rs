//! `delphi xenia admin ...` (specs/011 §3.2, §4.1, §8; plan milestone X8b): the command line over the X8 admin
//! library. Nothing here decides anything about trust; every judgement (is this epoch valid, did a quorum sign, is the
//! history sound) is the library's, and every signature is made behind the approval card.
//!
//! ```text
//! admin add <did> --person <p> --handle <h> [--as-admin] [--steward a,b|*]     propose a new member
//! admin remove <did> [--for-cause]                                             propose removing one
//! admin rotate <old-did> --to <new-did> --routine | --compromised              propose a key change (no default)
//! admin propose add|remove|rotate ...                                          the same, spelled out
//! admin show <n>                                                               who signed proposal n
//! admin sign <n>                                                               card, then sign proposal n
//! admin publish <n> [--validity-days D]                                        card, then publish epoch n
//! admin refresh [--validity-days D]                                            verify history, card, new statement
//! admin rewrite propose [--validity-days D]                                    author a rewrite statement (rewrite_of)
//! admin rewrite show <id> | sign <id> | publish <id>                           the same 2-of-3 flow, for a rewrite
//! admin genesis draft|sign|publish ...                                         the founders' ceremony
//! ```
//!
//! **Humans sign; agents don't.** [`TtyAdminApprover`] needs a terminal on stdin and stdout and the card's code typed
//! on it ([`crate::xenia_cmd::tty_confirm`]); no flag, variable or pipe answers. The key is this install's xenia key
//! and it is read here, by the process the operator started, and handed to the library as a parameter.

use crate::xenia_cmd::{key_path, now_secs, parse, pick, tty_confirm, Parsed};
use crate::home;
use delphi_core::xenia_admin::{self, Admin, AdminApprover, AdminDecision, AdminError, AdminPreview, GenesisDraft, ProposalReport};
use delphi_core::xenia_admin_card::{card, code, inline};
use delphi_core::xenia_store::Store;
use delphi_core::xenia_sync::VerifyConfig;
use delphi_xenia::admin::{self as lib, NewMember, Operation, ProposalSig, Rotation};
use delphi_xenia::identity::Identity;
use std::path::Path;

const USAGE: &str = "usage:
  delphi xenia admin add <did> --person <slug> --handle <h> [--as-admin] [--steward a,b|*]
  delphi xenia admin remove <did> [--for-cause]
  delphi xenia admin rotate <old-did> --to <new-did> --routine | --compromised
  delphi xenia admin propose add|remove|rotate ...      (same as the three above)
  delphi xenia admin show <n>        delphi xenia admin sign <n>
  delphi xenia admin publish <n> [--validity-days D]     delphi xenia admin refresh [--validity-days D]
  delphi xenia admin rewrite propose [--validity-days D]   delphi xenia admin rewrite show <id>
  delphi xenia admin rewrite sign <id>                      delphi xenia admin rewrite publish <id>
  delphi xenia admin genesis draft --org <o> --store <s> --founder <did>,<person>,<handle>[,admin|member[,areas]]... [--out <file>]
  delphi xenia admin genesis sign <draft> --org <o> --store <s> [--out <file>]
  delphi xenia admin genesis publish <draft> --url <store-url> --org <o> --store <s> --sig <file>... [--allow-local] [--validity-days D]
    [--store <org>/<store>] [--home <dir>]";

/// The real approver: a terminal on both ends and the typed code (a typed `yes` as well when there are warnings).
pub(crate) struct TtyAdminApprover;

impl AdminApprover for TtyAdminApprover {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        if tty_confirm(&code(p), p.flags.len()) { AdminDecision::Approve(p.approve(true)) } else { AdminDecision::Deny }
    }
}

/// Prints the card, then asks the inner approver. The card is printed here and nowhere else.
struct Carded<'a>(&'a mut dyn AdminApprover);

impl AdminApprover for Carded<'_> {
    fn decide(&mut self, p: &AdminPreview) -> AdminDecision {
        println!("{}", card(p));
        self.0.decide(p)
    }
}

/// Repeatable options (`--founder`, `--sig`) are pulled out first; everything else goes through the common parser.
fn split_multi(args: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let (mut multi, mut rest) = (Vec::new(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--founder" || a == "--sig" {
            if let Some(v) = it.next() {
                multi.push((a.trim_start_matches("--").to_string(), v.clone()));
                continue;
            }
        }
        rest.push(a.clone());
    }
    (multi, rest)
}

pub(crate) fn run(args: &[String], approver: &mut dyn AdminApprover) -> i32 {
    let Some(sub) = args.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if sub == "propose" {
        return match args.get(1).map(String::as_str) {
            Some(op @ ("add" | "remove" | "rotate")) => run(&[&[op.to_string()], &args[2..]].concat(), approver),
            _ => {
                eprintln!("delphi xenia admin propose: add, remove or rotate\n{USAGE}");
                2
            }
        };
    }
    let (multi, rest) = split_multi(&args[1..]);
    let p = match parse(&rest) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi xenia admin {sub}: {e}");
            return 2;
        }
    };
    let home = home::delphi_home(p.opts.get("home").map(String::as_str));
    match sub {
        "add" | "remove" | "rotate" => propose(&home, sub, &p),
        "show" => show(&home, &p),
        "sign" => sign(&home, &p, approver),
        "publish" => publish(&home, &p, approver),
        "refresh" => refresh(&home, &p, approver),
        "genesis" => genesis(&home, &p, &multi, approver),
        "rewrite" => rewrite(&home, &p, approver),
        other => {
            eprintln!("delphi xenia admin {other}: not a command\n{USAGE}");
            2
        }
    }
}

fn usage_err(sub: &str, m: &str) -> i32 {
    eprintln!("delphi xenia admin {sub}: {m}");
    2
}

fn fail(sub: &str, e: &AdminError) -> i32 {
    match e {
        AdminError::Denied => eprintln!("delphi xenia admin {sub}: cancelled; nothing was signed or sent"),
        e => eprintln!("delphi xenia admin {sub}: {e} [{}]", e.reason()),
    }
    1
}

fn validity(p: &Parsed) -> Result<i64, String> {
    match p.opts.get("validity-days") {
        None => Ok(lib::DEFAULT_VALIDITY_SECS),
        Some(d) => d.parse::<i64>().ok().filter(|d| *d > 0 && *d <= 3650).map(|d| d * 86400).ok_or_else(|| "--validity-days is a whole number of days".to_string()),
    }
}

fn key(home: &Path) -> Result<Identity, String> {
    Identity::load(&key_path(home)).map_err(|_| "this install has no key; `delphi xenia whoami` creates it, and an admin key is the one the roster lists".to_string())
}

fn n_of(p: &Parsed) -> Result<u64, String> {
    p.positional.first().and_then(|s| s.parse::<u64>().ok()).filter(|n| *n >= 2).ok_or_else(|| "needs the proposal number (an epoch, 2 or more)".to_string())
}

/// `add`, `remove`, `rotate`: write a draft epoch to `roster-proposal/<n>`. No key and no card: nothing is signed, and
/// every admin who is asked to sign it judges it afresh.
fn propose(home: &Path, sub: &str, p: &Parsed) -> i32 {
    let Some(target) = p.positional.first().cloned() else { return usage_err(sub, "needs a DID") };
    let op = match sub {
        "add" => {
            let (Some(person), Some(handle)) = (p.opts.get("person"), p.opts.get("handle")) else { return usage_err(sub, "needs --person and --handle") };
            let areas: Vec<String> = p.opts.get("steward").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
            Operation::Add(NewMember { did: target, person: person.clone(), handle: handle.clone(), admin: p.opts.contains_key("as-admin"), steward_areas: areas })
        }
        "remove" => Operation::Remove { did: target, for_cause: p.opts.contains_key("for-cause") },
        _ => {
            let Some(to) = p.opts.get("to") else { return usage_err(sub, "needs --to <new did>") };
            let kind = match Rotation::from_flags(p.opts.contains_key("routine"), p.opts.contains_key("compromised")) {
                Ok(k) => k,
                Err(m) => return usage_err(sub, m),
            };
            Operation::Rotate { old_did: target, new_did: to.clone(), kind }
        }
    };
    let (_, store) = match pick(home, p) {
        Ok(v) => v,
        Err(e) => return usage_err(sub, &e),
    };
    match Admin::new(&store, VerifyConfig::at(now_secs())).propose(op) {
        Ok(pr) => {
            println!("proposal {} written to branch {} (epoch hash {}); nothing is signed yet.", pr.report.n, pr.report.branch, pr.report.epoch_hash);
            print_report(&pr.report);
            println!("Next: each admin runs `delphi xenia admin sign {}`; when the quorum has signed, one runs `delphi xenia admin publish {}`.", pr.report.n, pr.report.n);
            0
        }
        Err(e) => fail(sub, &e),
    }
}

fn print_report(r: &ProposalReport) {
    for c in &r.changes {
        println!("  {}  {}  person {}  handle {}  {}", c.kind.slug(), inline(&c.did), inline(&c.person), inline(&c.handle), inline(&c.detail));
    }
    for f in &r.flags {
        println!("  WARNING {}: {}", f.slug(), f.text());
    }
    for m in &r.reminders {
        println!("  Note ({}): {}", m.slug(), m.text());
    }
    println!("  signatures: {} of {} needed{}", r.quorum.have, r.quorum.need, if r.quorum.people.is_empty() { String::new() } else { format!(" (signed: {})", r.quorum.people.iter().map(|x| inline(x)).collect::<Vec<_>>().join(", ")) });
}

fn show(home: &Path, p: &Parsed) -> i32 {
    let n = match n_of(p) {
        Ok(n) => n,
        Err(m) => return usage_err("show", &m),
    };
    let (_, store) = match pick(home, p) {
        Ok(v) => v,
        Err(e) => return usage_err("show", &e),
    };
    match Admin::new(&store, VerifyConfig::at(now_secs())).inspect(n) {
        Ok(r) => {
            println!("proposal {} on {} (epoch hash {})", r.n, r.branch, r.epoch_hash);
            print_report(&r);
            0
        }
        Err(e) => fail("show", &e),
    }
}

fn sign(home: &Path, p: &Parsed, approver: &mut dyn AdminApprover) -> i32 {
    let n = match n_of(p) {
        Ok(n) => n,
        Err(m) => return usage_err("sign", &m),
    };
    let (store, id) = match (pick(home, p), key(home)) {
        (Ok((_, s)), Ok(k)) => (s, k),
        (Err(e), _) | (_, Err(e)) => return usage_err("sign", &e),
    };
    match Admin::new(&store, VerifyConfig::at(now_secs())).sign(&id, n, &mut Carded(approver)) {
        Ok(d) => {
            println!("signature for proposal {n} {} (commit {}). When enough admins have signed, one runs `delphi xenia admin publish {n}`.", if d.already_there { "was already on the branch" } else { "pushed to the proposal branch" }, &d.commit[..d.commit.len().min(12)]);
            0
        }
        Err(e) => fail("sign", &e),
    }
}

fn published(what: &str, d: &xenia_admin::Delivered) {
    println!("{what}: {} file(s) in commit {}{}. Other installs accept it at their next sync.", d.paths.len(), &d.commit[..d.commit.len().min(12)], if d.already_there { " (the store already held exactly these bytes)" } else { "" });
}

fn publish(home: &Path, p: &Parsed, approver: &mut dyn AdminApprover) -> i32 {
    let (n, secs) = match (n_of(p), validity(p)) {
        (Ok(n), Ok(s)) => (n, s),
        (Err(m), _) | (_, Err(m)) => return usage_err("publish", &m),
    };
    let (store, id) = match (pick(home, p), key(home)) {
        (Ok((_, s)), Ok(k)) => (s, k),
        (Err(e), _) | (_, Err(e)) => return usage_err("publish", &e),
    };
    match Admin::new(&store, VerifyConfig::at(now_secs())).publish(&id, n, secs, &mut Carded(approver)) {
        Ok(d) => {
            published(&format!("roster epoch {n} published with its freshness statement"), &d);
            0
        }
        Err(e) => fail("publish", &e),
    }
}

fn refresh(home: &Path, p: &Parsed, approver: &mut dyn AdminApprover) -> i32 {
    let secs = match validity(p) {
        Ok(s) => s,
        Err(m) => return usage_err("refresh", &m),
    };
    let (store, id) = match (pick(home, p), key(home)) {
        (Ok((_, s)), Ok(k)) => (s, k),
        (Err(e), _) | (_, Err(e)) => return usage_err("refresh", &e),
    };
    match Admin::new(&store, VerifyConfig::at(now_secs())).refresh(&id, secs, &mut Carded(approver)) {
        Ok(d) => {
            published("freshness statement refreshed", &d);
            0
        }
        Err(e) => fail("refresh", &e),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Rewrite recovery (spec 011 section 8.4): the authoring side of `sync --accept-rewritten-head`

fn print_rewrite(r: &xenia_admin::RewriteReport) {
    let v = &r.view;
    println!("rewrite proposal {} on {} (statement expires {})", r.id, r.branch, inline(&r.expires_at));
    println!("  rewritten head (store_head): {}", inline(&v.rewritten));
    println!("  replaced head (rewrite_of):  {}", inline(&v.replaced));
    println!("  {} signed file(s) an install holding the replaced head will be asked to give up; {} history-rule violation(s) between the heads", v.affected.len(), v.rule_findings);
    for (why, path) in v.affected.iter().take(20) {
        println!("    {:<24} {}", inline(why), inline(path));
    }
    if v.affected.len() > 20 {
        println!("    ... and {} more", v.affected.len() - 20);
    }
    let q = &r.quorum;
    println!("  signatures: {} of {} admin people needed{}", q.have, q.need, if q.people.is_empty() { String::new() } else { format!(" (signed: {})", q.people.iter().map(|x| inline(x)).collect::<Vec<_>>().join(", ")) });
}

/// `rewrite propose|show|sign|publish`. Every step is judged by the library against what the host serves now.
fn rewrite(home: &Path, p: &Parsed, approver: &mut dyn AdminApprover) -> i32 {
    let Some(step) = p.positional.first().map(String::as_str) else { return usage_err("rewrite", "propose, show, sign or publish") };
    let id = || p.positional.get(1).cloned().ok_or_else(|| "needs the proposal id (printed by `rewrite propose`)".to_string());
    let (_, store) = match pick(home, p) {
        Ok(v) => v,
        Err(e) => return usage_err("rewrite", &e),
    };
    let admin = Admin::new(&store, VerifyConfig::at(now_secs()));
    let sub = format!("rewrite {step}");
    match step {
        "propose" => {
            let (secs, id) = match (validity(p), key(home)) {
                (Ok(s), Ok(k)) => (s, k),
                (Err(m), _) | (_, Err(m)) => return usage_err(&sub, &m),
            };
            match admin.rewrite_propose(&id, secs) {
                Ok(pr) => {
                    println!("rewrite proposal {} written to branch {}; nothing is signed yet.", pr.report.id, pr.report.branch);
                    print_rewrite(&pr.report);
                    println!("Next: a majority of the admin people each run `delphi xenia admin rewrite sign {}`; then one runs `delphi xenia admin rewrite publish {}`. Installs then run `delphi xenia sync --accept-rewritten-head`.", pr.report.id, pr.report.id);
                    0
                }
                Err(e) => fail(&sub, &e),
            }
        }
        "show" => match id().map(|i| admin.rewrite_inspect(&i)) {
            Ok(Ok(r)) => {
                print_rewrite(&r);
                0
            }
            Ok(Err(e)) => fail(&sub, &e),
            Err(m) => usage_err(&sub, &m),
        },
        "sign" => {
            let (id, k) = match (id(), key(home)) {
                (Ok(i), Ok(k)) => (i, k),
                (Err(m), _) | (_, Err(m)) => return usage_err(&sub, &m),
            };
            match admin.rewrite_sign(&k, &id, &mut Carded(approver)) {
                Ok(d) => {
                    println!("signature for rewrite proposal {id} {} (commit {}). When a majority of the admin people have signed, one runs `delphi xenia admin rewrite publish {id}`.", if d.already_there { "was already on the branch" } else { "pushed to the proposal branch" }, &d.commit[..d.commit.len().min(12)]);
                    0
                }
                Err(e) => fail(&sub, &e),
            }
        }
        "publish" => {
            let (id, k) = match (id(), key(home)) {
                (Ok(i), Ok(k)) => (i, k),
                (Err(m), _) | (_, Err(m)) => return usage_err(&sub, &m),
            };
            match admin.rewrite_publish(&k, &id, &mut Carded(approver)) {
                Ok(d) => {
                    published("rewrite statement published", &d);
                    println!("Installs run `delphi xenia sync --accept-rewritten-head`; each lists every affected path and its operator must accept.");
                    0
                }
                Err(e) => fail(&sub, &e),
            }
        }
        other => usage_err("rewrite", &format!("`{other}` is not propose, show, sign or publish")),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Genesis

#[cfg(test)]
#[path = "xenia_admin_cmd_tests.rs"]
mod tests;

const MAX_DRAFT: u64 = 256 * 1024;

fn read_capped(path: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_DRAFT {
        return Err(format!("{path}: not a regular file of at most {MAX_DRAFT} bytes"));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

/// `did,person,handle[,admin|member[,areas]]`; areas are `a|b` or `*`.
fn founder(spec: &str) -> Result<NewMember, String> {
    let f: Vec<&str> = spec.split(',').collect();
    if f.len() < 3 || f.len() > 5 {
        return Err(format!("--founder `{spec}`: expected <did>,<person>,<handle>[,admin|member[,areas]]"));
    }
    let admin = match f.get(3).copied() {
        None | Some("member") => false,
        Some("admin") => true,
        Some(o) => return Err(format!("--founder role `{o}` is admin or member")),
    };
    let steward_areas = f.get(4).map(|a| a.split('|').filter(|x| !x.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    Ok(NewMember { did: f[0].into(), person: f[1].into(), handle: f[2].into(), admin, steward_areas })
}

fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("{path}: {e} (an existing file is never overwritten)"))?;
    f.write_all(bytes).map_err(|e| format!("{path}: {e}"))
}

fn genesis(home: &Path, p: &Parsed, multi: &[(String, String)], approver: &mut dyn AdminApprover) -> i32 {
    let Some(step) = p.positional.first().map(String::as_str) else { return usage_err("genesis", "draft, sign or publish") };
    let (Some(org), Some(store_name)) = (p.opts.get("org"), p.opts.get("store")) else { return usage_err("genesis", "needs --org and --store") };
    let val = |k: &str| multi.iter().filter(|(n, _)| n == k).map(|(_, v)| v.clone()).collect::<Vec<_>>();
    match step {
        "draft" => {
            let members: Result<Vec<NewMember>, String> = val("founder").iter().map(|s| founder(s)).collect();
            let members = match members {
                Ok(m) if !m.is_empty() => m,
                Ok(_) => return usage_err("genesis draft", "needs at least one --founder"),
                Err(e) => return usage_err("genesis draft", &e),
            };
            let now = delphi_xenia::time::format_utc(now_secs());
            let d = match GenesisDraft::build(org, store_name, &members, &now) {
                Ok(d) => d,
                Err(e) => return fail("genesis draft", &e),
            };
            match p.opts.get("out") {
                Some(out) => {
                    if let Err(e) = write_new(out, d.bytes()) {
                        return usage_err("genesis draft", &e);
                    }
                    println!("draft written to {out}; hash {}. Send the file to every founder and confirm the hash with each, out of band.", d.hash());
                }
                None => print!("{}", String::from_utf8_lossy(d.bytes())),
            }
            0
        }
        "sign" => {
            let Some(file) = p.positional.get(1) else { return usage_err("genesis sign", "needs the draft file") };
            let (bytes, id) = match (read_capped(file), key(home)) {
                (Ok(b), Ok(k)) => (b, k),
                (Err(e), _) | (_, Err(e)) => return usage_err("genesis sign", &e),
            };
            let draft = match GenesisDraft::from_bytes(org, store_name, &bytes) {
                Ok(d) => d,
                Err(e) => return fail("genesis sign", &e),
            };
            println!("draft hash {} (confirm it with the admin who sent it)", draft.hash());
            let prepared = match xenia_admin::prepare_genesis_signature(&draft, &id) {
                Ok(p) => p,
                Err(e) => return fail("genesis sign", &e),
            };
            let signed = match Carded(approver).decide(prepared.preview()) {
                AdminDecision::Approve(a) => match prepared.sign(&a, &id) {
                    Ok(s) => s,
                    Err(e) => return fail("genesis sign", &e),
                },
                AdminDecision::Deny => return fail("genesis sign", &AdminError::Denied),
            };
            let out = p.opts.get("out").cloned().unwrap_or_else(|| format!("{file}.{}.sig.json", &id.fingerprint()[..12]));
            match write_new(&out, &signed.proposal_sig().to_json()) {
                Ok(()) => {
                    println!("signature written to {out}; send it to the founder who publishes the store.");
                    0
                }
                Err(e) => usage_err("genesis sign", &e),
            }
        }
        "publish" => {
            let (Some(file), Some(url)) = (p.positional.get(1), p.opts.get("url")) else { return usage_err("genesis publish", "needs the draft file and --url <store url>") };
            let secs = match validity(p) {
                Ok(s) => s,
                Err(m) => return usage_err("genesis publish", &m),
            };
            let (bytes, id) = match (read_capped(file), key(home)) {
                (Ok(b), Ok(k)) => (b, k),
                (Err(e), _) | (_, Err(e)) => return usage_err("genesis publish", &e),
            };
            let draft = match GenesisDraft::from_bytes(org, store_name, &bytes) {
                Ok(d) => d,
                Err(e) => return fail("genesis publish", &e),
            };
            let mut sigs: Vec<ProposalSig> = Vec::new();
            for f in val("sig") {
                match read_capped(&f).and_then(|b| ProposalSig::parse(&b).map_err(|e| format!("{f}: {e}"))) {
                    Ok(s) => sigs.push(s),
                    Err(e) => return usage_err("genesis publish", &e),
                }
            }
            let local = p.opts.contains_key("allow-local");
            let store = match Store::open(home, org, store_name, url, local) {
                Ok(s) => s,
                Err(e) => return usage_err("genesis publish", &format!("{e}{}", if local { "" } else { " (a local path needs --allow-local)" })),
            };
            match Admin::new(&store, VerifyConfig::at(now_secs())).init_store(&draft, &sigs, &id, secs, &mut Carded(approver)) {
                Ok(out) => {
                    println!("store {org}/{store_name} created at {} (head {}).", out.join.url, &out.head[..out.head.len().min(12)]);
                    println!("Read this to every member over a channel that is not the store, and join yourself with it:\n  {} --org {org} --store {store_name}", out.join.command());
                    0
                }
                Err(e) => fail("genesis publish", &e),
            }
        }
        other => usage_err("genesis", &format!("`{other}` is not draft, sign or publish")),
    }
}
