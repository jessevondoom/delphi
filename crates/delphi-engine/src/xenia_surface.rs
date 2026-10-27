//! What the console is shown about Xenia (plan milestone X9b): `xenia.status` and `xenia.pending`.
//!
//! Both are **reads**, served on the console socket only (`need_console` in `service.rs`; the Narrator and relay lists
//! do not name them and no agent tool reaches them). Neither can approve, sign or change anything: the steward's card
//! says which terminal command to run, and the approval itself stays in the terminal approver (`specs/011` §7.3: the
//! steward signs after seeing the exact record, without a model turn in between).
//!
//! Everything in the answer is a count, a fixed slug, or store text that `delphi_core::xenia_status::shown` already made
//! one short inert line. The console draws it as text.

use crate::kb::Kb;
use crate::xenia_read::KbSimilar;
use delphi_core::xenia_publish::{SimilarQuery, SimilarRecords};
use delphi_core::xenia_status::{self as xs, shown, StoreStatus};
use delphi_core::xenia_sync::VerifyConfig;
use serde_json::{json, Value};
use std::path::Path;

/// How many similar records a card shows.
const SIMILAR: usize = 3;

/// `xenia.status`: every joined store's report, and which are the operator's to act on.
pub fn status_value(home: &Path, now: i64) -> Value {
    let all: Vec<StoreStatus> = xs::status_all(home, &VerifyConfig::at(now));
    json!({"stores": all})
}

/// `xenia.pending`: the cards a steward has to review, with the similar records already in the store, and the command
/// that opens the terminal approver. `approve_in_console` is always false: it is part of the contract.
pub fn pending_value(home: &Path, ws: &Path, now: i64) -> Value {
    let cfg = VerifyConfig::at(now);
    let all = xs::status_all(home, &cfg);
    let me = xs::my_did(home);
    let db = ws.join("local/state.db");
    let kb = db.is_file().then(|| Kb::open(&db).ok()).flatten();
    let (mut cards, mut total, mut stores) = (Vec::new(), 0usize, Vec::new());
    for s in &all {
        if !s.me.as_ref().is_some_and(|m| m.steward) {
            continue;
        }
        let Some(did) = me.as_deref() else { continue };
        stores.push(format!("{}/{}", s.org, s.store));
        let pending = xs::pending_for(home, &s.org, &s.store, &cfg, did);
        total += pending.total;
        for c in pending.shown {
            let similar: Vec<Value> = kb
                .as_ref()
                .map(|kb| {
                    KbSimilar(kb)
                        .similar(&SimilarQuery { title: &c.title, summary: &c.summary, tags: &c.tags }, SIMILAR + 1)
                        .into_iter()
                        .filter(|h| h.id != c.id)
                        .take(SIMILAR)
                        .map(|h| json!({"id": shown(&h.id, 40), "title": shown(&h.title, xs::TITLE_CAP), "tier": shown(&h.tier, 4)}))
                        .collect()
                })
                .unwrap_or_default();
            let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
            v["similar"] = json!(similar);
            cards.push(v);
        }
    }
    json!({"cards": cards, "total": total, "stores": stores, "approve_in_console": false, "how": "Review the record, then run the command on its card in a terminal; the signing prompt shows the exact record and nothing is signed here."})
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::xenia_fixture::*;

    /// An agent reaches Delphi's functions only through the tool registry. None of the console's Xenia methods is a tool
    /// (and none may be added as one by accident): the only Xenia tool is the Green `xenia_search` over L2 summaries.
    #[test]
    fn no_agent_tool_reaches_the_xenia_surfaces() {
        let reg = crate::tools::Registry::builtin();
        for write in [false, true] {
            let names: Vec<String> = reg.defs(write).into_iter().map(|d| d.name).collect();
            let xenia: Vec<&String> = names.iter().filter(|n| n.contains("xenia")).collect();
            assert_eq!(xenia, vec!["xenia_search"], "{names:?}");
            for banned in ["xenia.status", "xenia.pending", "xenia.quarantine", "xenia_status", "xenia_pending", "xenia_quarantine", "xenia_endorse", "xenia_sync"] {
                assert!(reg.get(banned).is_none(), "{banned} must not be a tool");
            }
        }
    }

    #[test]
    fn pending_is_read_only_for_a_steward_and_status_lists_the_store() {
        let tmp = tempfile::tempdir().unwrap();
        let fx0 = Fx::with_cast(tmp.path());
        let m = records::memory_with(&fx0.id("mia"), &u(0), 1, "active", "v", "rust", "internal_empirical");
        let (fx, _) = Fx::installed_on(fx0, &[&m]);
        let home = fx.dir.join("home");
        let key = home.join("xenia/identity.key");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        fx.id("dave").save_new_for_tests(&key);
        let v = pending_value(&home, tmp.path(), now());
        assert_eq!((v["total"].clone(), v["approve_in_console"].clone()), (json!(1), json!(false)));
        let card = &v["cards"][0];
        assert!(card["command"].as_str().is_some_and(|c| c.starts_with("delphi xenia endorse ")), "{card}");
        for field in ["title", "author_handle", "tags", "summary", "scan_flags", "similar", "id", "rev"] {
            assert!(card.get(field).is_some(), "{field} is on the card: {card}");
        }
        let s = status_value(&home, now());
        assert_eq!(s["stores"][0]["org"], "acme");
        assert!(!s.to_string().contains("did:key"), "no key material in the answer");
    }
}
