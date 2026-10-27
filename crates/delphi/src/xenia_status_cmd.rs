//! `delphi xenia status`, and the Xenia parts of `delphi doctor` and `delphi needs` (plan milestone X9b).
//!
//! All three are read-only, offline and model-free: they call `delphi_core::xenia_status` and print what it returns.
//! Everything printed is a fixed word, a number, or store text that module already made safe (`shown`). The JSON is
//! the same struct, serialized: it holds no key material.

use crate::report::Level;
use delphi_core::xenia_status::{self as xs, Capped, StoreStatus};
use delphi_core::xenia_sync::VerifyConfig;
use std::path::Path;

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `delphi xenia status [--json] [--home <dir>]`.
pub fn run(home: &Path, args: &[String]) -> i32 {
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--home" => {
                it.next();
            }
            other => {
                eprintln!("delphi xenia status: unknown argument `{}`\nusage: delphi xenia status [--json] [--home <dir>]", xs::shown(other, 40));
                return 2;
            }
        }
    }
    let all = xs::status_all(home, &VerifyConfig::at(now_secs()));
    if json {
        match serde_json::to_string_pretty(&serde_json::json!({ "stores": all })) {
            Ok(t) => println!("{t}"),
            Err(e) => {
                eprintln!("delphi xenia status: {e}");
                return 1;
            }
        }
    } else {
        print!("{}", render(&all, now_secs()));
    }
    0
}

fn age(secs: i64) -> String {
    match secs.max(0) {
        s if s < 90 => format!("{s}s"),
        s if s < 90 * 60 => format!("{}m", s / 60),
        s if s < 48 * 3600 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

fn more<T>(c: &Capped<T>) -> String {
    if c.more() > 0 { format!(" and {} more", c.more()) } else { String::new() }
}

fn short(h: &str) -> &str {
    &h[..12.min(h.len())]
}

/// The human report. Pure, so the tests can feed it a hostile struct.
pub fn render(all: &[StoreStatus], now: i64) -> String {
    if all.is_empty() {
        return "no joined Xenia store (join one with `delphi xenia join`)\n".to_string();
    }
    let mut o = String::new();
    for s in all {
        o.push_str(&format!("xenia: {}/{}\n", s.org, s.store));
        for p in &s.problems {
            o.push_str(&format!("  PROBLEM   {}: {}\n", p.file, p.slug));
        }
        match (&s.accepted_head, s.roster_epoch) {
            (Some(h), Some(e)) => o.push_str(&format!("  head      {} (roster epoch {}, {} admin(s), {} member(s))\n", short(h), e, s.admins.unwrap_or(0), s.members.unwrap_or(0))),
            (Some(h), None) => o.push_str(&format!("  head      {} (roster unreadable)\n", short(h))),
            (None, _) => o.push_str("  head      nothing accepted yet (run `delphi xenia sync`)\n"),
        }
        if let Some(f) = &s.freshness {
            if f.state == "fresh" {
                let tag = if f.warns() { "  <- over half its life; an admin should refresh it" } else { "" };
                o.push_str(&format!("  freshness fresh: issued {} ago, {}% of its life used, expires {}{}\n", age(f.age_secs), f.life_used_pct, f.expires_at, tag));
            } else {
                o.push_str(&format!("  freshness STALE ({}): no endorsement counts, so every record reads at most L1\n", f.reason));
            }
        }
        o.push_str(&format!("  records   L2 {} - L1 {} - L0 {} - hidden {}\n", s.tiers.l2, s.tiers.l1, s.tiers.l0, s.tiers.hidden));
        if s.contested.total > 0 {
            o.push_str(&format!("  contested {}: {}{}\n", s.contested.total, s.contested.shown.join(", "), more(&s.contested)));
        }
        if s.quarantined.total > 0 {
            let by: Vec<String> = s.quarantine_by_reason.iter().map(|(k, v)| format!("{k} x{v}")).collect();
            o.push_str(&format!("  quarantine {} file(s) ({})\n", s.quarantined.total, by.join(", ")));
            for q in &s.quarantined.shown {
                o.push_str(&format!("            {}  {}\n", q.reason, q.path));
            }
            if s.quarantined.more() > 0 {
                o.push_str(&format!("            and {} more\n", s.quarantined.more()));
            }
        }
        if let Some(n) = &s.last_sync {
            let detail = if n.reason.is_empty() { String::new() } else { format!(" ({})", n.reason) };
            let extra = if n.findings > 0 { format!(", {} path(s) affected", n.findings) } else { String::new() };
            o.push_str(&format!("  last sync {} {} ago{}{}\n", n.outcome, age(now - n.at), detail, extra));
            if n.outcome == "tampering" {
                o.push_str("            possible tampering: nothing was applied and the previous accepted state stands\n");
            }
        } else if s.accepted_head.is_some() {
            o.push_str("  last sync not recorded yet\n");
        }
        if let Some(d) = s.unreachable_days {
            o.push_str(&format!("  host      unreachable for {d} day(s); offline use continues from the accepted state\n"));
        }
        if let Some(r) = &s.rewrite_pending {
            o.push_str(&format!("  REWRITE   the host serves a rewritten history ({}); nothing was applied.\n            Review what it changes and approve with: {}\n", short(&r.fetched), r.command));
        }
        o.push('\n');
    }
    o
}

// ---------------------------------------------------------------------------------------------------------
// doctor

/// One finding for `delphi doctor`: level, store (or file) and a sentence made of fixed words and numbers.
pub struct Check {
    pub level: Level,
    pub location: String,
    pub message: String,
}

fn fix(file: &str, slug: &str) -> String {
    match (file, slug) {
        ("sticky" | "pins", "loosened-permissions") => format!("the {file} file is readable or writable by others (expected mode 0600): `chmod 600` it, then check `delphi xenia status`"),
        ("store", "not-joined") => "the store directory has no clone".to_string(),
        (_, "missing") => format!("the {file} file is missing: join the store again with `delphi xenia join`"),
        ("sticky" | "pins", _) => format!("the {file} file is damaged ({slug}); it is install state, so do not edit it: restore it from a backup or join the store again"),
        _ => format!("{file}: {slug}"),
    }
}

/// The doctor checks for every joined store (spec 011 §4.1, §3.2, §3.5). Offline; an unreachable host is a warning.
pub fn doctor_checks(home: &Path, now: i64) -> Vec<Check> {
    let all = xs::status_all(home, &VerifyConfig::at(now));
    let mut out = Vec::new();
    if let Some(bad) = xs::identity_problem(home) {
        out.push(Check { level: Level::Fail, location: "xenia/identity.key".into(), message: if bad == "loosened-permissions" { "the install key is readable by others (expected mode 0600)".into() } else { format!("the install key file is not usable ({bad})") } });
    }
    for s in &all {
        let loc = format!("{}/{}", s.org, s.store);
        let mut bad = false;
        for p in &s.problems {
            bad = true;
            let level = if matches!(p.slug.as_str(), "damaged" | "loosened-permissions" | "not-a-file" | "unreadable" | "missing" | "bad-field" | "not-joined") { Level::Fail } else { Level::Warn };
            out.push(Check { level, location: loc.clone(), message: fix(p.file, &p.slug) });
        }
        if let Some(n) = s.admins {
            if n < 3 {
                bad = true;
                out.push(Check { level: Level::Warn, location: loc.clone(), message: format!("only {n} admin(s): a roster change needs a majority of admins and losing one key can deadlock it; spec 011 recommends at least 3") });
            }
        }
        if let Some(f) = &s.freshness {
            if f.state == "stale" && f.reason != "nothing-accepted" {
                bad = true;
                out.push(Check { level: Level::Warn, location: loc.clone(), message: format!("the freshness statement is stale ({}): no endorsement counts and every record reads at most L1; an admin must run `delphi xenia admin refresh`", f.reason) });
            } else if f.warns() && f.state == "fresh" {
                bad = true;
                out.push(Check { level: Level::Warn, location: loc.clone(), message: format!("the freshness statement has used {}% of its life (expires {}); an admin should run `delphi xenia admin refresh`", f.life_used_pct, f.expires_at) });
            }
        }
        if let Some(n) = &s.last_sync {
            if n.outcome == "tampering" {
                bad = true;
                out.push(Check { level: Level::Fail, location: loc.clone(), message: format!("the last sync found possible tampering ({} path(s), {}); nothing was applied. See `delphi xenia status`", n.findings, n.reason) });
            } else if n.alarming() && s.rewrite_pending.is_none() {
                bad = true;
                out.push(Check { level: Level::Warn, location: loc.clone(), message: format!("the last sync was {} ({}); the accepted state is unchanged", n.outcome, n.reason) });
            }
        }
        if s.rewrite_pending.is_some() {
            bad = true;
            out.push(Check { level: Level::Warn, location: loc.clone(), message: "a rewritten history waits for your approval; review it with `delphi xenia status`".into() });
        }
        if let Some(d) = s.unreachable_days.filter(|d| *d >= xs::UNREACHABLE_WARN_DAYS) {
            bad = true;
            out.push(Check { level: Level::Warn, location: loc.clone(), message: format!("the store host has been unreachable for {d} day(s); offline use is fine, but the freshness statement will lapse") });
        }
        if !bad {
            out.push(Check { level: Level::Ok, location: loc, message: format!("{} admin(s), epoch {}, freshness {}, {} quarantined file(s)", s.admins.unwrap_or(0), s.roster_epoch.unwrap_or(0), s.freshness.as_ref().map(|f| f.state).unwrap_or("unknown"), s.quarantined.total) });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------
// needs

/// What waits on this operator, for `delphi needs`: (evidence line, anything waiting?).
pub fn needs_evidence(home: &Path, now: i64) -> (String, bool) {
    let cfg = VerifyConfig::at(now);
    let all = xs::status_all(home, &cfg);
    if all.is_empty() {
        return ("no joined Xenia store".to_string(), false);
    }
    let me = xs::my_did(home);
    let pending: Vec<usize> = all.iter().map(|s| me.as_deref().filter(|_| s.me.as_ref().is_some_and(|m| m.steward)).map(|d| xs::pending_for(home, &s.org, &s.store, &cfg, d).total).unwrap_or(0)).collect();
    let todos = xs::todos(&all, &pending);
    if todos.is_empty() {
        (format!("{} joined store(s); nothing waits on you", all.len()), false)
    } else {
        (todos.iter().map(|t| t.what.clone()).collect::<Vec<_>>().join("; "), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::xenia_fixture::*;
    use delphi_xenia::testkit::{ORG, STORE};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn hostile() -> StoreStatus {
        let evil = "\x1b[31mred\x1b]0;pwned\x07 \u{202e}x [a](http://evil.example)";
        let mut s = bare("acme", "main");
        s.quarantined = Capped { total: 10_000, shown: (0..xs::LIST_CAP).map(|i| xs::Quarantined { path: format!("records/memory/{i}.md"), reason: "not-tracked".into() }).collect() };
        s.quarantine_by_reason.insert("not-tracked".into(), 10_000);
        s.contested = Capped { total: 1, shown: vec![xs::shown(evil, 40)] };
        s
    }

    fn bare(org: &str, store: &str) -> StoreStatus {
        let tmp = tempfile::tempdir().unwrap();
        xs::status_of(tmp.path(), org, store, &VerifyConfig::at(0), None)
    }

    #[test]
    fn ten_thousand_quarantined_files_print_a_few_lines_and_say_and_n_more() {
        let t = render(&[hostile()], 0);
        assert!(t.lines().count() < 45, "{t}");
        assert!(t.contains("and 9975 more") && t.contains("10000 file(s)"), "{t}");
    }

    #[test]
    fn contested_ids_are_inert_in_the_human_report() {
        let t = render(&[hostile()], 0);
        assert!(!t.chars().any(|c| (c.is_control() && c != '\n') || c == '\u{202e}'), "{t:?}");
    }

    fn home(fx: &Fx) -> std::path::PathBuf {
        fx.dir.join("home")
    }

    #[test]
    fn status_reports_a_joined_store_and_the_json_has_no_key_material() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let all = xs::status_all(&home(&fx), &cfg());
        let t = render(&all, now());
        assert!(t.contains("xenia: acme/acme-main") && t.contains("3 admin(s)") && t.contains("freshness fresh"), "{t}");
        let j = serde_json::to_string(&serde_json::json!({ "stores": all })).unwrap();
        assert!(!j.contains("did:key") && !j.contains(&fx.org.fingerprint("alice")), "{j}");
        assert_eq!(run(&home(&fx), &["--json".into()]), 0);
        assert_eq!(run(&home(&fx), &["--nope".into()]), 2);
    }

    fn levels(c: &[Check]) -> Vec<(Level, String)> {
        c.iter().map(|c| (c.level, c.message.clone())).collect()
    }

    #[test]
    fn doctor_warns_on_fewer_than_three_admins_half_life_and_a_stale_statement() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let ok = doctor_checks(&home(&fx), now());
        assert!(ok.iter().all(|c| c.level == Level::Ok), "{:?}", levels(&ok));
        let half = doctor_checks(&home(&fx), now() + 4 * DAY);
        assert!(half.iter().any(|c| c.level == Level::Warn && c.message.contains("of its life")), "{:?}", levels(&half));
        let stale = doctor_checks(&home(&fx), now() + 9 * DAY);
        assert!(stale.iter().any(|c| c.level == Level::Warn && c.message.contains("stale (expired)")), "{:?}", levels(&stale));
        assert!(stale.iter().all(|c| c.level != Level::Fail), "a lapsed statement is a warning, never a failure");
    }

    #[test]
    fn doctor_fails_on_damaged_or_loosened_state_and_names_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        let pins = fx.store.paths.state.join("pins.toml");
        fs::set_permissions(&pins, fs::Permissions::from_mode(0o644)).unwrap();
        let c = doctor_checks(&home(&fx), now());
        assert!(c.iter().any(|c| c.level == Level::Fail && c.message.contains("pins file") && c.message.contains("0600")), "{:?}", levels(&c));
        fs::set_permissions(&pins, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&fx.store.paths.sticky, b"\x00\x01garbage").unwrap();
        fs::set_permissions(&fx.store.paths.sticky, fs::Permissions::from_mode(0o600)).unwrap();
        let c = doctor_checks(&home(&fx), now());
        assert!(c.iter().any(|c| c.level == Level::Fail && c.message.contains("sticky file is damaged")), "{:?}", levels(&c));
    }

    #[test]
    fn doctor_warns_about_an_unreachable_store_and_never_fails_for_being_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let (fx, _) = Fx::installed(tmp.path(), &[]);
        fs::rename(fx.remote.host_path(), tmp.path().join("gone")).unwrap();
        let later = now() + 8 * DAY;
        let _ = fx.sync_at(later);
        let c = doctor_checks(&home(&fx), later);
        assert!(c.iter().any(|c| c.level == Level::Warn && c.message.contains("unreachable for 8 day(s)")), "{:?}", levels(&c));
        assert!(c.iter().all(|c| c.level != Level::Fail), "{:?}", levels(&c));
    }

    #[test]
    fn doctor_fails_on_possible_tampering() {
        let tmp = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(tmp.path());
        let m = delphi_xenia::testkit_records::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "payments", "internal_empirical");
        let (fx, _) = Fx::installed_on(fx0, &[&m]);
        fx.remote.remove(&m.path);
        fx.commit("delete");
        let _ = fx.sync();
        let c = doctor_checks(&home(&fx), now());
        assert!(c.iter().any(|c| c.level == Level::Fail && c.message.contains("possible tampering")), "{:?}", levels(&c));
        let _ = (ORG, STORE);
    }

    #[test]
    fn needs_names_the_admin_chore_and_the_steward_review_and_nothing_for_others() {
        let tmp = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(tmp.path());
        let m = delphi_xenia::testkit_records::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "rust", "internal_empirical");
        let (fx, _) = Fx::installed_on(fx0, &[&m]);
        // No key file in this home: nobody is "me", so nothing is the operator's to do.
        let (e, waiting) = needs_evidence(&home(&fx), now());
        assert!(!waiting && e.contains("nothing waits on you"), "{e}");
        let (e, waiting) = needs_evidence(tmp.path().join("empty-home").as_path(), now());
        assert!(!waiting && e == "no joined Xenia store", "{e}");
        // Dave's key as the install key: he is the steward of `rust` and one record awaits him.
        let key = home(&fx).join("xenia/identity.key");
        fs::create_dir_all(key.parent().unwrap()).unwrap();
        fx.id("dave").save_new_for_tests(&key);
        let (e, waiting) = needs_evidence(&home(&fx), now());
        assert!(waiting && e.contains("1 record(s) await your endorsement"), "{e}");
    }
}
