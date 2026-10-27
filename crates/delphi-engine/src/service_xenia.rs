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
            Err(e) => json!({"ok": false, "reason": e.reason(), "message": e.to_string(), "help": delphi_core::xenia_help::what_to_do(e.reason())}),
        })
    }
}
