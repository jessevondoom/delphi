//! `/decisions` and `delphi decisions`: the operator's review of an optimistic build's provisional decisions
//! (`specs/015`). Reviewing is what lets the merge happen: gate 6 stays red until every decision that allowed
//! something is ratified, and a rejection stops the merge.

use delphi_core::client::{Client, ClientError};
use serde_json::{json, Value};
use std::path::Path;

/// The one call this module makes, so tests can stand in for the daemon.
pub trait Rpc {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError>;
}

impl Rpc for Client {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        Client::call(self, method, params)
    }
}

fn err_text(e: &ClientError) -> String {
    match e {
        ClientError::Rpc { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n - 1).collect::<String>()) } else { one }
}

fn state(d: &Value) -> String {
    match (d["outcome"].as_str(), d["review"]["resolution"].as_str()) {
        (Some("deny"), _) => "refused".into(),
        _ if d["reverted"] == true => "reverted".into(),
        (_, Some(r)) => r.to_string(),
        _ => "PENDING".into(),
    }
}

/// What to look at first: pending decisions nobody second-checked, then other pending elevated ones, then the
/// rest of the pending ones, then everything already reviewed or refused. Within a group, the order they were made.
fn priority(d: &Value) -> u8 {
    match (state(d).as_str(), d["second"].as_str(), d["risk"].as_str()) {
        ("PENDING", Some("unseconded"), _) => 0,
        ("PENDING", _, Some("elevated")) => 1,
        ("PENDING", _, _) => 2,
        _ => 3,
    }
}

/// The ledger as a short table: what was decided, how risky, and what is waiting for the operator.
pub fn render_list(v: &Value) -> String {
    let mut ds = v["decisions"].as_array().cloned().unwrap_or_default();
    ds.sort_by_key(priority);
    if ds.is_empty() {
        return "no provisional decisions in this mission (they exist only for builds started with --optimistic)\n".into();
    }
    let pending = v["pending"].as_u64().unwrap_or(0);
    let mut out = format!("{} provisional decision(s), {} awaiting your review\n", ds.len(), pending);
    for d in &ds {
        out.push_str(&format!("  {:<14} {:<7} {:<8} {:<9} {}\n", d["decision_id"].as_str().unwrap_or("?"), d["task_id"].as_str().unwrap_or("?"), state(d), d["risk"].as_str().unwrap_or("?"), short(d["summary"].as_str().unwrap_or(""), 60)));
        if state(d) == "PENDING" || d["outcome"] == "deny" {
            let who = match (d["decided_by"].as_str().unwrap_or("rule"), d["second"].as_str().unwrap_or("")) {
                (by, "") => format!("by {}", by),
                (by, second) => format!("by {}; {}", by, second.replace("concurred:", "second seat: ")),
            };
            out.push_str(&format!("      why: {} ({})\n", short(d["rationale"].as_str().unwrap_or(""), 110), who));
        }
    }
    if pending > 0 {
        out.push_str("review with: /decisions show <id> · /decisions ratify <id…|all-routine> · /decisions reject <id> [note]\n");
    }
    out
}

fn render_one(d: &Value) -> String {
    let mut out = format!("{}  ({}, {} by {})\n  {}\n  tool: {} · risk: {} · decided: {}\n  why: {}\n  payload: blake3 {}\n", d["decision_id"].as_str().unwrap_or("?"), d["task_id"].as_str().unwrap_or("?"), d["outcome"].as_str().unwrap_or("?"), d["role"].as_str().unwrap_or("?"), short(d["summary"].as_str().unwrap_or(""), 160), d["tool"].as_str().unwrap_or("?"), d["risk"].as_str().unwrap_or("?"), state(d), short(d["rationale"].as_str().unwrap_or(""), 300), d["payload_blake3"].as_str().unwrap_or("?"));
    for e in d["effects"].as_array().into_iter().flatten() {
        out.push_str(&format!("  effect: {} {} ({} bytes)\n", e["kind"].as_str().unwrap_or("?"), e["path"].as_str().unwrap_or("?"), e["bytes"]));
    }
    if let Some(r) = d["review"].as_object() {
        out.push_str(&format!("  review: {} by {}{}\n", r.get("resolution").and_then(|v| v.as_str()).unwrap_or("?"), r.get("by").and_then(|v| v.as_str()).unwrap_or("?"), r.get("note").and_then(|v| v.as_str()).filter(|n| !n.is_empty()).map(|n| format!(" — {}", n)).unwrap_or_default()));
    }
    out
}

/// Run one `/decisions` command and return what to print.
pub fn handle(rpc: &mut dyn Rpc, mission: &str, args: &[String]) -> String {
    let list = match rpc.call("decisions.list", json!({"mission_id": mission})) {
        Ok(v) => v,
        Err(e) => return format!("{}\n", err_text(&e)),
    };
    let ds = list["decisions"].as_array().cloned().unwrap_or_default();
    let find = |id: &str| ds.iter().find(|d| d["decision_id"] == id).cloned();
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => render_list(&list),
        Some("show") => match args.get(1).and_then(|id| find(id)) {
            Some(d) => render_one(&d),
            None => "usage: /decisions show <decision id> (ids are listed by /decisions)\n".into(),
        },
        Some("ratify") => {
            let ids: Vec<String> = if args.get(1).map(|s| s.as_str()) == Some("all-routine") {
                ds.iter().filter(|d| d["outcome"] == "allow" && d["review"].is_null() && d["risk"] == "routine").filter_map(|d| d["decision_id"].as_str().map(String::from)).collect()
            } else {
                args[1.min(args.len())..].to_vec()
            };
            if ids.is_empty() {
                return "nothing to ratify. Name decision ids, or `all-routine` for every pending ordinary edit (elevated ones are reviewed one by one).\n".into();
            }
            review_all(rpc, mission, &ids, "ratified", "", &find)
        }
        Some("reject") => match args.get(1) {
            Some(id) => review_all(rpc, mission, std::slice::from_ref(id), "rejected", &args[2..].join(" "), &find),
            None => "usage: /decisions reject <decision id> [why]\n".into(),
        },
        Some(other) => format!("unknown /decisions command `{}` (list, show, ratify, reject)\n", other),
    }
}

fn review_all(rpc: &mut dyn Rpc, mission: &str, ids: &[String], resolution: &str, note: &str, find: &dyn Fn(&str) -> Option<Value>) -> String {
    let mut out = String::new();
    for id in ids {
        let Some(d) = find(id) else {
            out.push_str(&format!("{}: no such decision\n", id));
            continue;
        };
        // The review names the exact payload that was shown, so a decision that changed since cannot be ratified blind.
        match rpc.call("decisions.review", json!({"mission_id": mission, "decision_id": id, "payload_blake3": d["payload_blake3"], "resolution": resolution, "note": note})) {
            Ok(_) => out.push_str(&format!("{}: {}\n", id, resolution)),
            Err(e) => out.push_str(&format!("{}: {}\n", id, err_text(&e))),
        }
    }
    out
}

/// `delphi decisions …`: the same commands from a shell. Attaches like `delphi run` does (a daemon this process
/// started, or one it can reach from a terminal).
pub fn cli(home: &Path, ws_dir: &Path, mission: Option<String>, args: &[String]) -> i32 {
    use delphi_core::spawn::{connect_or_spawn, SpawnOptions};
    let Some(exe) = crate::chat::daemon_exe() else {
        eprintln!("delphi decisions: cannot locate this program to start the daemon");
        return 4;
    };
    let mut attached = match connect_or_spawn(&SpawnOptions { home, workspace: ws_dir, daemon_exe: &exe, extra_args: vec![], prefix_args: vec!["__core".into()], own_version: Some(env!("CARGO_PKG_VERSION")) }) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi decisions: {}", err_text(&e));
            return 4;
        }
    };
    let mission_id = match attached.client.call("mission.open", json!({"mission_id": mission})) {
        Ok(v) => v["mission_id"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            eprintln!("delphi decisions: {}", err_text(&e));
            return 4;
        }
    };
    print!("{}", handle(&mut attached.client, &mission_id, args));
    if attached.spawned {
        let _ = attached.client.call("daemon.shutdown", json!({}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        list: Value,
        calls: Vec<(String, Value)>,
        fail_review: Option<String>,
    }

    impl Rpc for Fake {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
            self.calls.push((method.into(), params.clone()));
            match method {
                "decisions.list" => Ok(self.list.clone()),
                "decisions.review" => match &self.fail_review {
                    Some(m) => Err(ClientError::Rpc { code: -32602, message: m.clone(), data: None }),
                    None => Ok(json!({"decision_id": params["decision_id"], "resolution": params["resolution"]})),
                },
                _ => Err(ClientError::Closed),
            }
        }
    }

    fn d(id: &str, outcome: &str, risk: &str, review: Option<&str>) -> Value {
        json!({"decision_id": id, "task_id": "t_0001", "role": "builder", "tool": "write_file", "summary": format!("write src/{}.rs (3 bytes)", id), "payload_blake3": format!("hash_{}", id), "outcome": outcome, "rationale": "an ordinary edit", "risk": risk, "effects": [{"kind": "worktree_edit", "path": format!("@worktree/src/{}.rs", id), "bytes": 3}], "review": review.map(|r| json!({"resolution": r, "note": "", "by": "console-1"}))})
    }

    fn fake() -> Fake {
        let ds = vec![d("dec_a", "allow", "routine", None), d("dec_b", "allow", "elevated", None), d("dec_c", "allow", "routine", Some("ratified")), d("dec_d", "deny", "elevated", None)];
        Fake { list: json!({"decisions": ds, "pending": 2}), calls: vec![], fail_review: None }
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_list_shows_state_risk_and_what_is_waiting_and_says_what_to_do() {
        let out = handle(&mut fake(), "m1", &[]);
        assert!(out.contains("4 provisional decision(s), 2 awaiting your review"), "{}", out);
        assert!(out.contains("dec_a") && out.contains("PENDING") && out.contains("ratified") && out.contains("refused") && out.contains("elevated"), "{}", out);
        assert!(out.contains("/decisions ratify"), "{}", out);
        assert!(handle(&mut Fake { list: json!({"decisions": [], "pending": 0}), calls: vec![], fail_review: None }, "m1", &[]).contains("only for builds started with --optimistic"));
    }

    #[test]
    fn what_nobody_second_checked_comes_first_and_the_list_says_who_decided() {
        let mut un = d("dec_u", "allow", "elevated", None);
        un["second"] = json!("unseconded");
        un["decided_by"] = json!("strategist");
        let mut conc = d("dec_k", "allow", "elevated", None);
        conc["second"] = json!("concurred:red-team");
        let list = json!({"decisions": [d("dec_r", "allow", "routine", Some("ratified")), d("dec_p", "allow", "routine", None), conc, un], "pending": 3});
        let out = render_list(&list);
        let at = |id: &str| out.find(id).unwrap_or_else(|| panic!("{} missing in\n{}", id, out));
        assert!(at("dec_u") < at("dec_k") && at("dec_k") < at("dec_p") && at("dec_p") < at("dec_r"), "{}", out);
        assert!(out.contains("by strategist; unseconded") && out.contains("by rule; second seat: red-team"), "{}", out);
    }

    #[test]
    fn showing_a_decision_gives_the_effect_the_reason_and_the_hash() {
        let out = handle(&mut fake(), "m1", &args(&["show", "dec_c"]));
        assert!(out.contains("effect: worktree_edit @worktree/src/dec_c.rs") && out.contains("hash_dec_c") && out.contains("review: ratified by console-1"), "{}", out);
        assert!(handle(&mut fake(), "m1", &args(&["show", "nope"])).starts_with("usage:"));
    }

    #[test]
    fn ratifying_sends_the_hash_that_was_shown_and_all_routine_leaves_elevated_ones_for_a_closer_look() {
        let mut f = fake();
        let out = handle(&mut f, "m1", &args(&["ratify", "all-routine"]));
        assert_eq!(out, "dec_a: ratified\n", "only the pending routine one: not the elevated, the reviewed or the refused");
        let review = f.calls.iter().find(|(m, _)| m == "decisions.review").unwrap();
        assert_eq!((review.1["decision_id"].as_str(), review.1["payload_blake3"].as_str(), review.1["resolution"].as_str()), (Some("dec_a"), Some("hash_dec_a"), Some("ratified")));
        let mut f = fake();
        let out = handle(&mut f, "m1", &args(&["ratify", "dec_b", "dec_zzz"]));
        assert!(out.contains("dec_b: ratified") && out.contains("dec_zzz: no such decision"), "{}", out);
        assert!(handle(&mut fake(), "m1", &args(&["ratify"])).contains("nothing to ratify"));
    }

    #[test]
    fn rejecting_carries_the_operators_reason_and_a_refusal_from_the_daemon_is_shown() {
        let mut f = fake();
        let out = handle(&mut f, "m1", &args(&["reject", "dec_b", "touches", "the", "lockfile"]));
        assert_eq!(out, "dec_b: rejected\n");
        let review = f.calls.iter().find(|(m, _)| m == "decisions.review").unwrap();
        assert_eq!(review.1["note"], "touches the lockfile");
        let mut f = fake();
        f.fail_review = Some("`dec_a` changed after you reviewed it; review it again".into());
        assert!(handle(&mut f, "m1", &args(&["ratify", "dec_a"])).contains("changed after you reviewed it"));
        assert!(handle(&mut fake(), "m1", &args(&["reject"])).starts_with("usage:"));
        assert!(handle(&mut fake(), "m1", &args(&["frobnicate"])).contains("unknown /decisions command"));
    }

    #[test]
    fn a_daemon_that_cannot_list_is_reported_and_nothing_is_reviewed() {
        struct Down;
        impl Rpc for Down {
            fn call(&mut self, _: &str, _: Value) -> Result<Value, ClientError> {
                Err(ClientError::Closed)
            }
        }
        assert!(handle(&mut Down, "m1", &args(&["ratify", "dec_a"])).contains("closed the connection"));
    }
}
