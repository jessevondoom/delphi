//! `delphi forge ...` (spec 135): slice claims (F0) and the finisher (F1). No daemon; a process only with `finish --gates`
//! (F1b, `finisher_gates.rs`).

use crate::slice_claims::{self, Out};
use std::path::PathBuf;

/// Flags that take a value, so a value is never read as a positional word.
const VALUE_FLAGS: [&str; 6] = ["--repo", "--home", "--by", "--ttl", "--spec", "--slice"];

fn positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if VALUE_FLAGS.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(a.clone());
        }
    }
    out
}

fn has(args: &[String], f: &str) -> bool {
    args.iter().any(|a| a == f)
}

pub fn usage() -> &'static str {
    "usage: delphi forge claim <spec> <slice> [--by <holder>] [--ttl <dur>] [--take] [--local] [--repo <dir>] [--home <dir>]\n       delphi forge claim --spec-number [--by <holder>] [--ttl <dur>] [--local] [--repo <dir>] [--home <dir>]\n       delphi forge release <spec> <slice> [--by <holder>] [--local] [--repo <dir>] [--home <dir>]\n       delphi forge release --spec-number <NNN> [--by <holder>] [--repo <dir>]\n       delphi forge list [--all] [--json] [--sweep] [--local] [--repo <dir>] [--home <dir>]\n       delphi forge finish [--check] [--gates] [--spec <spec> --slice <slice>] [--body] [--repo <dir>] [--home <dir>]\n       delphi forge tracker [--spec <spec>] [--repo <dir>]"
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Runs a subcommand (the arguments start with it) and returns what it prints.
pub fn exec(args: &[String]) -> Out {
    let rest = &args[1.min(args.len())..];
    let repo = crate::flag(rest, "--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let home = crate::home::delphi_home(crate::flag(rest, "--home").as_deref());
    let by = crate::flag(rest, "--by");
    let pos = positionals(rest);
    let usage_err = |why: &str| Out { code: 2, out: String::new(), err: format!("delphi forge: {}\n{}", why, usage()) };
    match args.first().map(String::as_str) {
        Some("claim") => {
            let numbered = has(rest, "--spec-number");
            let ttl = match crate::flag(rest, "--ttl").map(|t| crate::run::parse_duration(&t)) {
                None if numbered => crate::claim_refs::SPEC_NUMBER_TTL_SECS,
                None => slice_claims::DEFAULT_TTL_SECS,
                Some(Ok(d)) => d.as_secs() as i64,
                Some(Err(e)) => return usage_err(&e),
            };
            if numbered {
                if !pos.is_empty() {
                    return usage_err("claim --spec-number takes no spec or slice");
                }
                return crate::claim_refs::reserve_spec_number(&repo, &home, by.as_deref(), ttl, has(rest, "--local"), now());
            }
            let [spec, slice] = &pos[..] else { return usage_err("claim takes a spec and a slice") };
            crate::claim_refs::claim(&slice_claims::ClaimReq { repo: &repo, home: &home, spec, slice, by: by.as_deref(), ttl_secs: ttl, take: has(rest, "--take"), now: now() }, has(rest, "--local"))
        }
        Some("release") if has(rest, "--spec-number") => {
            let [number] = &pos[..] else { return usage_err("release --spec-number takes the number") };
            crate::claim_refs::release_spec_number(&repo, number, by.as_deref(), now())
        }
        Some("release") => {
            let [spec, slice] = &pos[..] else { return usage_err("release takes a spec and a slice") };
            crate::claim_refs::release(&repo, &home, spec, slice, by.as_deref(), has(rest, "--local"), now())
        }
        Some("list") if pos.is_empty() => crate::claim_refs::list(&repo, &home, has(rest, "--all"), has(rest, "--json"), has(rest, "--sweep"), has(rest, "--local"), now()),
        Some("finish") if pos.is_empty() => {
            let (spec, slice) = (crate::flag(rest, "--spec"), crate::flag(rest, "--slice"));
            crate::finisher::finish(&crate::finisher::FinishReq { repo: &repo, home: &home, check: has(rest, "--check"), spec: spec.as_deref(), slice: slice.as_deref(), body: has(rest, "--body"), gates: has(rest, "--gates"), now: now() })
        }
        Some("tracker") if pos.is_empty() => tracker_view(&repo, crate::flag(rest, "--spec").as_deref()),
        _ => usage_err("name a subcommand"),
    }
}

/// `forge tracker` (§14.10): the table's rows and the per-slice files as one table, optionally of one spec (`135` or a stem).
fn tracker_view(repo: &std::path::Path, spec: Option<&str>) -> Out {
    let t = crate::tracker::load(repo);
    let num = spec.map(|s| crate::tracker::spec_number(s).unwrap_or_else(|| s.trim().to_string()));
    let rows: Vec<&crate::tracker::Row> = match &num {
        None => t.rows.iter().collect(),
        Some(n) => crate::tracker::rows_for(&t.rows, n),
    };
    let bad: Vec<(String, Vec<String>)> = t.bad.into_iter().filter(|(p, _)| num.as_ref().is_none_or(|n| p.contains(&format!("/{}/", n)))).collect();
    Out { code: 0, out: crate::tracker::render(&rows, &bad), err: String::new() }
}

/// The entry point from `main`: prints and returns the exit code.
pub fn cli(args: &[String]) -> i32 {
    let o = exec(args);
    if !o.out.is_empty() {
        print!("{}", o.out);
    }
    if !o.err.is_empty() {
        eprintln!("{}", o.err.trim_end());
    }
    o.code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn values_of_flags_are_not_positional() {
        assert_eq!(positionals(&v(&["135", "--by", "me", "F0", "--take", "--ttl", "2h"])), v(&["135", "F0"]));
    }

    #[test]
    fn misuse_is_exit_2() {
        assert_eq!(exec(&v(&["claim", "135"])).code, 2);
        assert_eq!(exec(&v(&["nope"])).code, 2);
        assert_eq!(exec(&v(&[])).code, 2);
        assert_eq!(exec(&v(&["claim", "135", "F0", "--ttl", "soon"])).code, 2);
        assert_eq!(exec(&v(&["claim", "135", "--spec-number"])).code, 2);
        assert_eq!(exec(&v(&["release", "--spec-number"])).code, 2);
    }
}
