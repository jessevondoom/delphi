//! The console's side of Xenia (`specs/011` §4.1, §7.3; plan milestone X9): `xenia.status` and the steward's attestation
//! card, `xenia.attest.prepare` and `xenia.attest.decide`.
//!
//! The card is the same one the CLI shows: the exact bytes that will be signed, the flags, the similar records and a
//! short code covering the digest. `prepare` parks the action at its card on its own thread (the gate's `Approver` waits
//! there); `decide` answers it, and only then does the gate sign, verify and push. What keeps "humans sign, agents
//! don't" true here is structural, as in the CLI:
//!
//! * both methods need the **console** scope, which an agent, the Narrator socket and the relay do not hold, and no
//!   tool in `tools.rs` reaches this code;
//! * `decide` takes the card's code **as typed by the operator**, so an answer for a card that is not the one on the
//!   screen is refused; a card that has flags needs them acknowledged in the same call;
//! * the key is read by the parked thread alone, from `$DELPHI_HOME/xenia/identity.key`, and is never returned,
//!   logged or put in an event;
//! * no model turn runs between the card and the signature (the gate guarantees it; nothing here adds one).

use super::*;
use delphi_core::xenia_action::{self, Action, Answer as CardAnswer, Pending, ScopeCtx};
use delphi_core::xenia_publish::{NoIndex, PublishError, Publisher, SimilarRecords};
use delphi_core::xenia_status;
use delphi_core::xenia_sync::VerifyConfig;
use delphi_core::xenia_action::Identity;

/// Cards waiting for an answer at one time. Opening another past this drops (and so denies) the oldest.
const MAX_PENDING: usize = 4;

/// A card on the screen.
pub(super) struct Parked {
    pending: Pending,
    store: xenia_status::StoreRef,
    word: &'static str,
    at: Instant,
}

fn now_unix() -> i64 {
    delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)
}

fn opts_from(p: &Value) -> Result<std::collections::BTreeMap<String, String>, RpcError> {
    let mut out = std::collections::BTreeMap::new();
    if let Some(o) = p["options"].as_object() {
        for (k, v) in o {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => return Err(err(code::INVALID_PARAMS, format!("option `{k}` is not text"))),
            };
            if k.len() > 40 || text.len() > 20_000 {
                return Err(err(code::INVALID_PARAMS, format!("option `{k}` is too long")));
            }
            out.insert(k.clone(), text);
        }
    }
    Ok(out)
}

impl EngineService {
    /// `xenia.status`: every joined store, as the CLI's `status` shows it. A read; no network and no model.
    pub(super) async fn xenia_status(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let home = self.engine.ws.home.clone();
        let only = p["store"].as_str().map(String::from);
        let now = now_unix();
        let out = tokio::task::spawn_blocking(move || {
            let cfg = VerifyConfig::at(now);
            let mut stores = Vec::new();
            for (r, st) in xenia_status::all(&home, &cfg) {
                if only.as_deref().is_some_and(|o| o != r.id()) {
                    continue;
                }
                match st {
                    Ok(st) => stores.push(json!({"store": r.id(), "text": xenia_status::render(&st, now), "status": xenia_status::to_json(&st)})),
                    Err(e) => stores.push(json!({"store": r.id(), "text": format!("{} cannot be read: {}", r.id(), e), "error": e})),
                }
            }
            stores
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?;
        Ok(json!({ "stores": out }))
    }

    /// `xenia.attest.prepare` `{action, target?, options?, store?}`: build the action, run the gate up to its card, and
    /// return the card. Nothing is signed or sent. Refusals that happen before there is anything to show (a rule, a scan
    /// block, an unreachable store) come back as `{ok: false, reason, message}`.
    pub(super) async fn xenia_attest_prepare(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let word = p["action"].as_str().unwrap_or("");
        let Some(word) = Action::WORDS.iter().find(|w| **w == word).copied() else {
            return Err(err(code::INVALID_PARAMS, format!("`action` is one of: {}", Action::WORDS.join(", "))));
        };
        let opts = opts_from(&p)?;
        let (home, ws) = (self.engine.ws.home.clone(), self.engine.ws.dir.clone());
        let selector = p["store"].as_str().map(String::from);
        let target = p["target"].as_str().map(String::from);
        let store_ref = xenia_status::pick_store(&home, selector.as_deref()).map_err(|m| err(code::INVALID_PARAMS, m))?;
        let ctx_scope = ScopeCtx { home: home.clone(), workspace: Some(ws.clone()), today: delphi_core::time::now_rfc3339()[..10].to_string() };
        let action = tokio::task::spawn_blocking({
            let (opts, target) = (opts.clone(), target.clone());
            move || Action::build(word, target.as_deref(), &opts, &ctx_scope)
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?
        .map_err(|m| err(code::INVALID_PARAMS, m))?;
        let title = action.title();
        let key_path = home.join("xenia").join("identity.key");
        let job_store = store_ref.clone();
        let job_home = home.clone();
        let now = now_unix();
        let begun = tokio::task::spawn_blocking(move || {
            xenia_action::begin(
                move |approver| {
                    let key = Identity::load(&key_path).map_err(|_| PublishError::Io("this install has no Xenia key yet: run `delphi xenia whoami`".into()))?;
                    let store = xenia_status::open(&job_home, &job_store).map_err(|e| PublishError::Io(e.to_string()))?;
                    // "Similar records already in the store" comes from the workspace's index when it has one.
                    let kb = Some(ws.join("local/state.db")).filter(|d| d.is_file()).and_then(|d| crate::kb::Kb::open(&d).ok());
                    let kb_similar = kb.as_ref().map(crate::xenia_read::KbSimilar);
                    let similar: &dyn SimilarRecords = match &kb_similar {
                        Some(k) => k,
                        None => &NoIndex,
                    };
                    let publisher = Publisher::new(&store, VerifyConfig::at(now), similar);
                    let result = action.run(&publisher, &key, approver);
                    xenia_status::record_publish(&store, word, &result, now);
                    result
                },
                xenia_action::WAIT,
            )
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?;
        match begun {
            Err(e) => Ok(json!({"ok": false, "reason": e.reason(), "message": e.to_string()})),
            Ok(pending) => {
                let card = xenia_action::card(&title, &pending.preview);
                let code_text = xenia_action::code(&pending.preview);
                let (digest, flags) = (pending.preview.digest.clone(), pending.preview.flags.len());
                let mut parked = self.attest.lock().unwrap();
                parked.retain(|_, v| v.at.elapsed() < xenia_action::WAIT);
                while parked.len() >= MAX_PENDING {
                    let oldest = parked.iter().min_by_key(|(_, v)| v.at).map(|(k, _)| k.clone());
                    match oldest {
                        Some(k) => parked.remove(&k),
                        None => break,
                    };
                }
                parked.insert(code_text.clone(), Parked { pending, store: store_ref.clone(), word, at: Instant::now() });
                Ok(json!({"ok": true, "code": code_text, "digest": digest, "flags": flags, "card": card, "store": store_ref.id(), "expires_secs": xenia_action::WAIT.as_secs()}))
            }
        }
    }

    /// `xenia.attest.decide` `{code, decision: "sign"|"cancel", flags_acknowledged?, mission_id?}`.
    pub(super) async fn xenia_attest_decide(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let code_text = p["code"].as_str().unwrap_or("").trim().to_string();
        let sign = match p["decision"].as_str() {
            Some("sign") => true,
            Some("cancel") => false,
            _ => return Err(err(code::INVALID_PARAMS, "`decision` is `sign` or `cancel`")),
        };
        let ack = p["flags_acknowledged"].as_bool().unwrap_or(false);
        let parked = {
            let mut map = self.attest.lock().unwrap();
            // A card that has flags is not consumed by a yes that did not acknowledge them: the operator may try again.
            if sign && map.get(&code_text).is_some_and(|v| !v.pending.preview.flags.is_empty()) && !ack {
                return Err(err(code::INVALID_PARAMS, "this card has flags (they do not block); acknowledge them with `flags_acknowledged: true` to sign"));
            }
            map.remove(&code_text)
        };
        let Some(parked) = parked.filter(|v| v.at.elapsed() < xenia_action::WAIT) else {
            return Err(err(code::INVALID_PARAMS, "no card with that code is waiting (it was answered, it expired, or the code was mistyped)"));
        };
        let (store, word) = (parked.store.clone(), parked.word);
        let result = tokio::task::spawn_blocking(move || parked.pending.decide(CardAnswer { approve: sign, flags_acknowledged: ack })).await.map_err(|e| err(-32000, e.to_string()))?;
        // The journal was written by the parked thread; tell the mission's log now, so the Narrator shows it.
        if let Ok(m) = self.mission(p["mission_id"].as_str()).await {
            let _ = self.engine.reconcile_xenia(&m).await;
        }
        Ok(match result {
            Ok(out) => json!({"ok": true, "store": store.id(), "action": word, "commit": out.commit, "files": out.paths, "already_there": out.already_there}),
            Err(PublishError::Denied) => json!({"ok": false, "reason": "denied", "message": "cancelled: nothing was signed or sent"}),
            Err(e) => json!({"ok": false, "reason": e.reason(), "message": e.to_string()}),
        })
    }
}

// ---------------------------------------------------------------------------------------------------------
// Limited admin signing (`delphi_core::xenia_admin_console` has the rules; this is the daemon's shell)

use delphi_core::xenia_admin_console as ac;

/// The one admin card on the screen.
pub(super) struct ParkedAdmin {
    pending: ac::Pending,
    store: xenia_status::StoreRef,
    kind: ac::Kind,
    flags: Vec<String>,
    at: Instant,
}

/// A value that may be pasted into a terminal command shown to the operator; anything else becomes a placeholder.
fn safe(v: &Value, placeholder: &str) -> String {
    match v.as_str() {
        Some(s) if !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'.' | b'_' | b'-' | b',' | b'*')) => s.to_string(),
        _ => placeholder.to_string(),
    }
}

/// The terminal command that does what was asked, for a refusal to name.
fn terminal_command(action: &str, p: &Value, store: &str) -> String {
    let op = &p["op"];
    let n = p["proposal"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| "<n>".into());
    match action {
        "sign" | "publish" => format!("delphi xenia admin {action} {n} --store {store}"),
        "refresh" => format!("delphi xenia admin refresh --store {store}"),
        "init" => "delphi xenia admin init draft --org <org> --store <store> --founder did=<did>,person=<person>,handle=<handle>,role=admin --out <file>".into(),
        "accept-rewritten-head" => format!("delphi xenia sync --accept-rewritten-head --store {store}"),
        "rotate" => format!("delphi xenia admin rotate {} {} --routine|--compromised --store {store}", safe(&op["old_did"], "<old-did>"), safe(&op["new_did"], "<new-did>")),
        _ => match op["kind"].as_str() {
            Some("remove") => format!("delphi xenia admin remove {}{} --store {store}", safe(&op["did"], "<did>"), if op["for_cause"].as_bool() == Some(true) { " --for-cause" } else { "" }),
            Some("rotate") => format!("delphi xenia admin rotate {} {} --routine|--compromised --store {store}", safe(&op["old_did"], "<old-did>"), safe(&op["new_did"], "<new-did>")),
            _ => format!("delphi xenia admin add {} --person {} --handle {}{} --store {store}", safe(&op["did"], "<did>"), safe(&op["person"], "<person>"), safe(&op["handle"], "<handle>"), if op["admin"].as_bool() == Some(true) { " --admin" } else { "" }),
        },
    }
}

fn refusal(reason: &str, slug: &str, command: &str, n: &str) -> Value {
    json!({"ok": false, "reason": reason, "message": ac::fill(slug, command, n), "command": command})
}

fn operation_from(op: &Value) -> Result<ac::Operation, RpcError> {
    use ac::{NewMember, Operation, Rotation};
    let text = |k: &str| op[k].as_str().map(String::from).ok_or_else(|| err(code::INVALID_PARAMS, format!("`op.{k}` is required")));
    match op["kind"].as_str() {
        Some("add") => Ok(Operation::Add(NewMember {
            did: text("did")?,
            person: text("person")?,
            handle: text("handle")?,
            admin: op["admin"].as_bool().unwrap_or(false),
            steward_areas: op["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        })),
        Some("remove") => Ok(Operation::Remove { did: text("did")?, for_cause: op["for_cause"].as_bool().unwrap_or(false) }),
        Some("rotate") => Ok(Operation::Rotate {
            old_did: text("old_did")?,
            new_did: text("new_did")?,
            kind: match op["rotation"].as_str() {
                Some("routine") => Rotation::Routine,
                Some("compromised") => Rotation::Compromised,
                _ => return Err(err(code::INVALID_PARAMS, "`op.rotation` is `routine` or `compromised`: a rotation has no default")),
            },
        }),
        _ => Err(err(code::INVALID_PARAMS, "`op.kind` is add, remove or rotate")),
    }
}

impl EngineService {
    /// Console admin signatures recorded in the last hour across the joined stores.
    fn admin_signatures_last_hour(&self, now: i64) -> usize {
        let home = &self.engine.ws.home;
        let mut n = 0;
        for r in xenia_status::list_stores(home) {
            if let Ok(store) = xenia_status::open(home, &r) {
                let j = xenia_status::read_journal(&store);
                n += ac::recent_console_signatures(j.iter().map(|e| (e.kind.as_str(), e.outcome.as_str(), e.detail["via"].as_str().unwrap_or(""), e.at)), now);
            }
        }
        n
    }

    fn admin_journal(&self, r: &xenia_status::StoreRef, outcome: &str, head: Option<String>, detail: Value, now: i64) {
        if let Ok(store) = xenia_status::open(&self.engine.ws.home, r) {
            let mut d = detail;
            d["via"] = json!("console");
            let _ = xenia_status::record(&store, "admin", outcome, head, d, now);
        }
    }

    /// `xenia.admin.prepare` `{action, proposal?, op?, store?, validity_days?}`: classify, and for `sign`, `publish` and
    /// `refresh` return the card. `propose` runs at once (it needs no key and signs nothing). Everything that touches an
    /// admin, and genesis and rewrite recovery, comes back as `{ok: false, reason: "terminal-only", command}`.
    pub(super) async fn xenia_admin_prepare(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let action = p["action"].as_str().unwrap_or("").to_string();
        if !["propose", "sign", "publish", "refresh", "init", "rotate", "accept-rewritten-head"].contains(&action.as_str()) {
            return Err(err(code::INVALID_PARAMS, "`action` is propose, sign, publish, refresh, init, rotate or accept-rewritten-head"));
        }
        let home = self.engine.ws.home.clone();
        let store_ref = xenia_status::pick_store(&home, p["store"].as_str()).map_err(|m| err(code::INVALID_PARAMS, m))?;
        let command = terminal_command(&action, &p, &store_ref.id());
        let n_text = p["proposal"].as_u64().map(|n| n.to_string()).unwrap_or_default();
        // What can never be answered here.
        match action.as_str() {
            "init" => return Ok(refusal("terminal-only", "terminal-genesis", &command, &n_text)),
            "accept-rewritten-head" => return Ok(refusal("terminal-only", "terminal-rewrite", &command, &n_text)),
            "rotate" => return Ok(refusal("terminal-only", "terminal-rotation", &command, &n_text)),
            _ => {}
        }
        if !ac::console_admin_enabled(&home) {
            return Ok(refusal("switch-off", "switch-off", &command, &n_text));
        }
        let now = now_unix();
        if self.admin_signatures_last_hour(now) >= ac::MAX_PER_HOUR {
            return Ok(refusal("rate-limit", "rate-limit", &command, &n_text));
        }
        let validity = p["validity_days"].as_i64().filter(|d| (1..=60).contains(d)).map(|d| d * 86_400).unwrap_or(ac::DEFAULT_VALIDITY_SECS);
        if action == "propose" {
            let op = operation_from(&p["op"])?;
            let r = store_ref.clone();
            let h = home.clone();
            let out = tokio::task::spawn_blocking(move || -> Result<Value, Value> {
                let store = xenia_status::open(&h, &r).map_err(|e| json!({"ok": false, "reason": "store-unusable", "message": e.to_string()}))?;
                let admin = delphi_core::xenia_admin::Admin::new(&store, VerifyConfig::at(now));
                let (base, cand) = admin.operation_epochs(&op).map_err(|e| json!({"ok": false, "reason": e.reason(), "message": e.to_string()}))?;
                if let Err(t) = ac::classify(&base, &cand, admin.policy.min_admins) {
                    return Err(json!({"terminal": t.slug()}));
                }
                let prop = admin.propose(op).map_err(|e| json!({"ok": false, "reason": e.reason(), "message": e.to_string()}))?;
                Ok(json!({"ok": true, "proposal": prop.report.n, "branch": prop.report.branch, "epoch_hash": prop.report.epoch_hash, "commit": prop.commit,
                    "changes": prop.report.changes.iter().map(|c| json!({"kind": c.kind.slug(), "did": c.did, "person": c.person})).collect::<Vec<_>>(),
                    "flags": prop.report.flags.iter().map(|f| f.slug()).collect::<Vec<_>>(),
                    "next": format!("each admin signs it (`xenia.admin.prepare` sign {}), then one publishes it", prop.report.n)}))
            })
            .await
            .map_err(|e| err(-32000, e.to_string()))?;
            return match out {
                Ok(v) => {
                    self.admin_journal(&store_ref, "proposed", v["commit"].as_str().map(String::from), json!({"proposal": v["proposal"]}), now);
                    self.tell_xenia(&p).await;
                    Ok(v)
                }
                Err(v) if v["terminal"].is_string() => Ok(refusal("terminal-only", v["terminal"].as_str().unwrap_or(""), &command, &n_text)),
                Err(v) => Ok(v),
            };
        }
        let kind = match action.as_str() {
            "sign" | "publish" => {
                let Some(n) = p["proposal"].as_u64() else { return Err(err(code::INVALID_PARAMS, "`proposal` is the proposal number")) };
                if action == "sign" { ac::Kind::Sign(n) } else { ac::Kind::Publish(n, validity) }
            }
            _ => ac::Kind::Refresh(validity),
        };
        // One admin card at a time.
        {
            let mut slot = self.admin_card.lock().unwrap();
            if slot.as_ref().is_some_and(|c| c.at.elapsed() >= ac::WAIT) {
                *slot = None;
            }
            if slot.is_some() {
                return Ok(refusal("pending-card", "pending-card", &command, &n_text));
            }
        }
        let key_path = home.join("xenia").join("identity.key");
        let (job_store, job_home, job_kind) = (store_ref.clone(), home.clone(), kind.clone());
        let begun = tokio::task::spawn_blocking(move || {
            ac::begin(
                move |approver| {
                    let key = ac::Identity::load(&key_path).map_err(|_| ac::ConsoleError::NoKey)?;
                    let store = xenia_status::open(&job_home, &job_store).map_err(|e| ac::ConsoleError::Admin(delphi_core::xenia_admin::AdminError::Io(e.to_string())))?;
                    ac::run(&store, &key, VerifyConfig::at(now), &job_kind, approver)
                },
                ac::WAIT,
            )
        })
        .await
        .map_err(|e| err(-32000, e.to_string()))?;
        match begun {
            Err(e) => Ok(self.console_error(&e, &command, &n_text)),
            Ok(pending) => {
                let card = ac::card(&kind, &pending.preview);
                let code_text = ac::code(&pending.preview);
                let flags: Vec<String> = pending.preview.flags.iter().map(|f| f.slug().to_string()).collect();
                let digest = pending.preview.digest.clone();
                let mut slot = self.admin_card.lock().unwrap();
                if slot.is_some() {
                    return Ok(refusal("pending-card", "pending-card", &command, &n_text));
                }
                *slot = Some(ParkedAdmin { pending, store: store_ref.clone(), kind: kind.clone(), flags: flags.clone(), at: Instant::now() });
                Ok(json!({"ok": true, "code": code_text, "confirm": ac::confirm_word(&kind), "flags": flags, "digest": digest, "card": card, "store": store_ref.id(), "expires_secs": ac::WAIT.as_secs()}))
            }
        }
    }

    fn console_error(&self, e: &ac::ConsoleError, command: &str, n: &str) -> Value {
        match e {
            ac::ConsoleError::Terminal(t) => refusal("terminal-only", t.slug(), command, n),
            ac::ConsoleError::Stale => refusal("stale", "stale", command, n),
            ac::ConsoleError::NoKey => refusal("no-key", "no-key", command, n),
            ac::ConsoleError::Admin(delphi_core::xenia_admin::AdminError::NotAnApprover) => refusal("not-an-approver", "not-admin", command, n),
            ac::ConsoleError::Admin(delphi_core::xenia_admin::AdminError::Denied) => refusal("denied", "denied", command, n),
            ac::ConsoleError::Admin(a) => json!({"ok": false, "reason": a.reason(), "message": a.to_string(), "command": command}),
        }
    }

    async fn tell_xenia(&self, p: &Value) {
        if let Ok(m) = self.mission(p["mission_id"].as_str()).await {
            let _ = self.engine.reconcile_xenia(&m).await;
        }
    }

    /// `xenia.admin.decide` `{code, confirm, acknowledged: [flag names], decision: "sign"|"cancel", mission_id?}`.
    pub(super) async fn xenia_admin_decide(&self, p: Value, ctx: CallContext) -> RpcOutcome {
        need_console(&ctx)?;
        let code_text = p["code"].as_str().unwrap_or("").trim().to_string();
        let sign = match p["decision"].as_str() {
            Some("sign") => true,
            Some("cancel") => false,
            _ => return Err(err(code::INVALID_PARAMS, "`decision` is `sign` or `cancel`")),
        };
        let now = now_unix();
        let parked = {
            let mut slot = self.admin_card.lock().unwrap();
            let waiting = slot.as_ref().is_some_and(|c| c.at.elapsed() < ac::WAIT && ac::code(&c.pending.preview) == code_text);
            if !waiting {
                return Err(err(code::INVALID_PARAMS, ac::message("no-card")));
            }
            if sign {
                let c = slot.as_ref().expect("checked");
                if p["confirm"].as_str().map(str::trim) != Some(ac::confirm_word(&c.kind).as_str()) {
                    return Err(err(code::INVALID_PARAMS, ac::message("wrong-confirm")));
                }
                let said: std::collections::BTreeSet<String> = p["acknowledged"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let need: std::collections::BTreeSet<String> = c.flags.iter().cloned().collect();
                if said != need {
                    return Err(err(code::INVALID_PARAMS, ac::message("flags-unacknowledged")));
                }
                if self.admin_signatures_last_hour(now) >= ac::MAX_PER_HOUR {
                    return Err(err(code::INVALID_PARAMS, ac::fill("rate-limit", &format!("delphi xenia admin {} --store {}", c.kind.word(), c.store.id()), "")));
                }
            }
            slot.take().expect("checked")
        };
        let (store, kind) = (parked.store.clone(), parked.kind.clone());
        let n_text = kind.proposal().map(|n| n.to_string()).unwrap_or_default();
        let command = format!("delphi xenia admin {} {} --store {}", kind.word(), n_text, store.id());
        let result = tokio::task::spawn_blocking(move || parked.pending.decide(ac::Answer { approve: sign, flags_acknowledged: true })).await.map_err(|e| err(-32000, e.to_string()))?;
        let detail = json!({"proposal": kind.proposal(), "action": kind.word()});
        let out = match &result {
            Ok(d) => {
                let outcome = match kind {
                    ac::Kind::Sign(_) => "signed",
                    ac::Kind::Publish(..) => "published",
                    ac::Kind::Refresh(_) => "refreshed",
                };
                self.admin_journal(&store, outcome, Some(d.commit.clone()), detail, now);
                json!({"ok": true, "store": store.id(), "action": kind.word(), "commit": d.commit, "already_there": d.already_there})
            }
            Err(ac::ConsoleError::Admin(delphi_core::xenia_admin::AdminError::Denied)) => {
                self.admin_journal(&store, "denied", None, detail, now);
                self.console_error(result.as_ref().err().expect("err"), &command, &n_text)
            }
            Err(e) => {
                self.admin_journal(&store, if matches!(e, ac::ConsoleError::Stale) { "stale" } else { "refused" }, None, detail, now);
                self.console_error(e, &command, &n_text)
            }
        };
        self.tell_xenia(&p).await;
        Ok(out)
    }
}
