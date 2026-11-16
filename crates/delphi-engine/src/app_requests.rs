//! The `request` grant (`specs/120` §4.2): the four things the desktop app may ask the daemon to start.
//!
//! | Method | What it starts | What it takes |
//! | :-- | :-- | :-- |
//! | `app.research_continue` | a further run on a research topic (`specs/112` §5) | `topic_id`; `section` (a heading's anchor); `note` (typed) |
//! | `app.research_ask` | research on a follow-up question | `question` (typed); `follows` (a topic id) |
//! | `app.page_rebuild` | a rebuild of a topic page | `slug` |
//! | `app.page_dispute` | a dispute on a claim | `claim`; `note` (typed) |
//!
//! Each is something the operator could start at a terminal without being asked anything (`delphi research continue`,
//! `/research`, `delphi pages rebuild`, `delphi pages dispute`), and each runs the same code as that command, as the
//! operator's, under the same budgets and limits. What is particular to the app is here:
//!
//! * **Ids and typed text only** (item 1). The parameters are a fixed list ([`PARAMS`]); a parameter outside it, or one that
//!   is not a string, refuses the call. An id is checked for its shape and then looked up by the daemon. The one thing a
//!   request carries that came from a page is a section's anchor: letters, digits and hyphens that must be the anchor of a
//!   heading on a page that follows the topic. Nothing else a document says can be put in a request.
//! * **Limits** (item 3): [`PER_HOUR`] requests an hour for a key and [`AT_ONCE`] running at once, each refused by name.
//! * **The record** (item 4): an `app.request` event for each request that was counted, with the key's name, the method,
//!   the ids and the request's state, written again when the state changes. Typed text is never in it.
//!
//! Only [`delphi_core::server::Extension::app_request`] reaches this, and the server calls that only for an app session
//! whose key holds the grant at that moment. The server runs it as a task of its own, so a key revoked while a request is
//! being answered does not cut the request halfway; and nothing here holds work inside the call: a research run is queued
//! in the one queue, a rebuild is its own task.

use super::{err, EngineService};
use delphi_core::event::EventEnvelope;
use delphi_core::proto::code;
use delphi_core::server::RpcOutcome;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// The four methods, in the grant table's order ([`delphi_core::app_keys::REQUEST_METHODS`]; a test holds the two equal).
pub const METHODS: [&str; 4] = ["app.research_continue", "app.research_ask", "app.page_rebuild", "app.page_dispute"];
/// What each method takes, and nothing else (AC-7). Every value is a string.
pub const PARAMS: [(&str, &[&str]); 4] = [("app.research_continue", &["topic_id", "section", "note"]), ("app.research_ask", &["question", "follows"]), ("app.page_rebuild", &["slug"]), ("app.page_dispute", &["claim", "note"])];
/// The parameters that hold what the operator typed. Every other parameter is an id.
pub const TYPED: [&str; 2] = ["note", "question"];
/// Requests an hour for one key (§4.2 item 3).
pub const PER_HOUR: usize = 30;
/// Requests of one key running at once.
pub const AT_ONCE: usize = 4;
/// The most typed text a request may carry, in characters (§4.2 item 5). The command each request stands for has a tighter
/// bound of its own, which still applies.
pub const TEXT_MAX: usize = 2_000;
/// The longest section anchor.
pub const ANCHOR_MAX: usize = 80;
/// The longest question, as `research.start` takes it.
const QUESTION_MAX: usize = 200;
/// The states an `app.request` event names.
pub const STATES: [&str; 6] = ["queued", "running", "done", "failed", "cancelled", "refused"];

/// Something a request started that has not ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    /// A research run, by its id in the one queue.
    Research(String),
    /// A rebuild of this topic's page.
    Rebuild(String),
}

/// What each key asked in the last hour and what of it still runs. In memory: a daemon that restarts forgets the hour's
/// count (the work it had started is gone with it).
#[derive(Default)]
pub struct Tally {
    asked: HashMap<String, VecDeque<i64>>,
    running: HashMap<String, Vec<Work>>,
    made: u64,
}

impl Tally {
    /// Count one request of `key` at `now`, or say which limit refuses it. `going` says whether a piece of work still runs.
    pub fn admit(&mut self, key: &str, now: i64, going: &dyn Fn(&Work) -> bool) -> Result<(), String> {
        let asked = self.asked.entry(key.to_string()).or_default();
        while asked.front().is_some_and(|t| now - *t >= 3600) {
            asked.pop_front();
        }
        if asked.len() >= PER_HOUR {
            return Err(format!("refused: the limit is {} requests an hour for one key, and this key has made {} in the last hour", PER_HOUR, asked.len()));
        }
        let running = self.running.entry(key.to_string()).or_default();
        running.retain(|w| going(w));
        if running.len() >= AT_ONCE {
            return Err(format!("refused: the limit is {} requests running at once for one key; wait for one to end", AT_ONCE));
        }
        asked.push_back(now);
        Ok(())
    }

    fn begin(&mut self, key: &str, work: Work) {
        self.running.entry(key.to_string()).or_default().push(work);
    }

    fn end(&mut self, key: &str, work: &Work) {
        if let Some(r) = self.running.get_mut(key) {
            r.retain(|w| w != work);
        }
    }

    /// Is some key's request rebuilding this page now?
    fn rebuilding(&self, slug: &str) -> bool {
        self.running.values().flatten().any(|w| matches!(w, Work::Rebuild(s) if s == slug))
    }

    fn next_id(&mut self, now_ms: i64) -> String {
        self.made += 1;
        format!("aq-{:x}-{}", now_ms, self.made)
    }
}

/// A request's parameters after the shape check: each is absent or a string.
pub struct Asked<'a>(&'a Value);

impl<'a> Asked<'a> {
    fn get(&self, k: &str) -> Option<&'a str> {
        self.0[k].as_str()
    }
}

/// Is `s` a heading's anchor as the page check makes one (`library_page_check::slug_of`): lowercase letters, digits and hyphens?
pub fn valid_anchor(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= ANCHOR_MAX && s.chars().all(|c| c == '-' || (c.is_alphanumeric() && c.to_lowercase().eq(std::iter::once(c))))
}

/// The shape check (§4.2 item 1, AC-7): an object holding only the method's own parameters, each a string, typed text within
/// [`TEXT_MAX`]. `Err` is what the caller is told; it never repeats a value.
pub fn check<'a>(method: &str, p: &'a Value) -> Result<Asked<'a>, String> {
    let (_, allowed) = PARAMS.iter().find(|(m, _)| *m == method).ok_or("no such request")?;
    let obj = p.as_object().ok_or("a request's parameters are one object")?;
    for (k, v) in obj {
        if !allowed.contains(&k.as_str()) {
            let shown: String = k.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(24).collect();
            return Err(format!("`{}` takes {} and nothing else; `{}` is not one of them", method, allowed.join(", "), shown));
        }
        let Some(s) = v.as_str() else { return Err(format!("`{}` must be a string", k)) };
        if TYPED.contains(&k.as_str()) && s.chars().count() > TEXT_MAX {
            return Err(format!("`{}` is at most {} characters", k, TEXT_MAX));
        }
    }
    Ok(Asked(p))
}

/// A daemon message as an event may carry it: one line, no control character, at most 200 characters.
fn why(message: &str) -> String {
    message.chars().map(|c| if c.is_control() { ' ' } else { c }).take(200).collect::<String>().trim().to_string()
}

/// A running rebuild's place among its key's four: given back when dropped.
struct Slot {
    svc: Arc<EngineService>,
    key: String,
    work: Work,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut tally) = self.svc.app_asked.lock() {
            tally.end(&self.key, &self.work);
        }
    }
}

/// One request that was counted: who asked, what, and the ids it named.
struct Req {
    id: String,
    key: String,
    method: String,
    ids: Value,
}

impl EngineService {
    /// Answer one of the `request` grant's methods for the key named `key`. `None` for any other method.
    pub async fn app_request(self: &Arc<Self>, key: &str, method: &str, params: Value) -> Option<RpcOutcome> {
        if !METHODS.contains(&method) || !delphi_core::app_keys::is_name(key) {
            return None;
        }
        Some(self.app_request_checked(key, method, params).await)
    }

    async fn app_request_checked(self: &Arc<Self>, key: &str, method: &str, params: Value) -> RpcOutcome {
        let bad = |m: String| err(code::INVALID_PARAMS, m);
        let p = check(method, &params).map_err(bad)?;
        // Every id is checked for its shape before anything is counted or looked up: a malformed call starts nothing and costs nothing.
        let ids = match method {
            "app.research_continue" => {
                let id = p.get("topic_id").unwrap_or("");
                if !crate::research_record::valid_topic_id(id) {
                    return Err(bad("`topic_id` is a research topic's id".into()));
                }
                match p.get("section") {
                    Some(a) if !valid_anchor(a) => return Err(bad("`section` is a heading's anchor: lowercase letters, digits and hyphens".into())),
                    Some(a) => json!({"topic_id": id, "section": a}),
                    None => json!({"topic_id": id}),
                }
            }
            "app.research_ask" => match p.get("follows") {
                Some(f) if !crate::research_record::valid_topic_id(f) => return Err(bad("`follows` is a research topic's id".into())),
                Some(f) => json!({"follows": f}),
                None => json!({}),
            },
            "app.page_rebuild" => {
                let slug = p.get("slug").unwrap_or("");
                if !crate::library::topics::valid_slug(slug) {
                    return Err(bad("`slug` is a topic page's slug".into()));
                }
                json!({"slug": slug})
            }
            _ => {
                let claim = p.get("claim").unwrap_or("");
                if claim.is_empty() || claim.len() > 120 || !claim.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) || claim.contains("..") {
                    return Err(bad("`claim` is a claim's id".into()));
                }
                json!({"claim": claim})
            }
        };
        let now = (self.engine.clock_unix)();
        let req = {
            let mut tally = self.app_asked.lock().unwrap();
            let going = |w: &Work| match w {
                Work::Research(run) => self.research_jobs.status(run, crate::research_console::SESSION).is_some_and(|s| matches!(s["status"].as_str(), Some("queued" | "running"))),
                Work::Rebuild(_) => true,
            };
            tally.admit(key, now, &going).map_err(|m| err(code::FORBIDDEN, m))?;
            Req { id: tally.next_id(crate::host_rpc::now_ms()), key: key.to_string(), method: method.to_string(), ids }
        };
        // A request is the operator at work, as the same command at a terminal is.
        self.last_operator.store(now, std::sync::atomic::Ordering::SeqCst);
        let started = match method {
            "app.research_continue" => self.app_continue(&req, &p).await,
            "app.research_ask" => self.app_ask(&req, &p).await,
            "app.page_rebuild" => self.app_rebuild(&req).await,
            _ => self.app_dispute(&req, &p).await,
        };
        match started {
            Ok(v) => Ok(v),
            Err(e) => {
                self.app_event(&req, "refused", Some(&e.message)).await;
                Err(e)
            }
        }
    }

    /// Write the request's state as an `app.request` event of the default mission, where `delphi app log` and every attached
    /// client read it. The payload is the key's name, the method, the ids and the state: never what was typed.
    async fn app_event(&self, req: &Req, state: &str, because: Option<&str>) {
        debug_assert!(STATES.contains(&state));
        let mut payload = json!({"name": req.key, "request": req.id, "method": req.method, "ids": req.ids, "state": state});
        if let Some(b) = because {
            payload["why"] = json!(why(b));
        }
        if let Ok(m) = self.mission_at(None, None).await {
            let _ = m.append(EventEnvelope::new(&m.id, crate::app_audit::REQUEST_EVENT, payload)).await;
        }
    }

    /// `app.research_continue`: `delphi research continue <topic> [--note …]`. A section is named in the run's note by its
    /// anchor, after the daemon has found a heading with that anchor on a page that follows the topic.
    async fn app_continue(self: &Arc<Self>, req: &Req, p: &Asked<'_>) -> RpcOutcome {
        let id = req.ids["topic_id"].as_str().unwrap_or("").to_string();
        let mut note = p.get("note").map(str::trim).unwrap_or("").to_string();
        if let Some(anchor) = req.ids["section"].as_str() {
            let (home, tid, a) = (self.engine.ws.home.clone(), id.clone(), anchor.to_string());
            let found = tokio::task::spawn_blocking(move || crate::library::Library::in_home(&home).topic_has_section(&tid, &a)).await.map_err(|e| err(code::INTERNAL, e.to_string()))?;
            if !found {
                return Err(err(code::NOT_FOUND, "no page that follows this topic has a section with that anchor"));
            }
            note = format!("{}{}(section: {})", note, if note.is_empty() { "" } else { " " }, anchor);
        }
        let mut params = json!({"topic_id": id});
        if !note.is_empty() {
            params["note"] = json!(note);
        }
        let out = self.continue_as_operator(&params).await?;
        self.app_research_started(req, out).await
    }

    /// `app.research_ask`: the console's `/research <question>`. A question that follows a topic is researched in that
    /// topic's project.
    async fn app_ask(self: &Arc<Self>, req: &Req, p: &Asked<'_>) -> RpcOutcome {
        let question = p.get("question").map(str::trim).unwrap_or("").to_string();
        if question.is_empty() || question.chars().count() > QUESTION_MAX || question.chars().any(char::is_control) {
            return Err(err(code::INVALID_PARAMS, format!("`question` is one line of 1 to {} characters", QUESTION_MAX)));
        }
        let project = match req.ids["follows"].as_str() {
            None => None,
            Some(f) => {
                let (wsd, f) = (self.engine.ws.dir.clone(), f.to_string());
                let found = tokio::task::spawn_blocking(move || {
                    let topic = crate::research_record::find(&wsd, &f)?;
                    Some(crate::scope::project_list(&wsd).into_iter().map(|(_, root)| root).find(|root| crate::research_record::topics_dir(root).join(&topic.id) == topic.dir))
                })
                .await
                .map_err(|e| err(code::INTERNAL, e.to_string()))?;
                found.ok_or_else(|| err(code::NOT_FOUND, "no research topic has the id `follows` names"))?
            }
        };
        let out = self.launch_research(question, project, None, None, false).await?;
        self.app_research_started(req, out).await
    }

    /// A run was queued: record it, say so, and follow it to its end in a task of its own.
    async fn app_research_started(self: &Arc<Self>, req: &Req, out: Value) -> RpcOutcome {
        let run = out["research_id"].as_str().unwrap_or("").to_string();
        let state_of = |svc: &EngineService| svc.research_jobs.status(&run, crate::research_console::SESSION).and_then(|s| s["status"].as_str().map(String::from));
        let work = Work::Research(run.clone());
        self.app_asked.lock().unwrap().begin(&req.key, work.clone());
        let first = match state_of(self).as_deref() {
            Some("running") => "running",
            _ => "queued",
        };
        self.app_event(req, first, None).await;
        let (svc, req2, mut last) = (self.clone(), Req { id: req.id.clone(), key: req.key.clone(), method: req.method.clone(), ids: req.ids.clone() }, first.to_string());
        let watched = run.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(svc.app_poll()).await;
                let now = svc.research_jobs.status(&watched, crate::research_console::SESSION).and_then(|s| s["status"].as_str().map(String::from));
                let state = match now.as_deref() {
                    Some("queued") => "queued",
                    Some("running") => "running",
                    Some("failed") => "failed",
                    Some("cancelled") => "cancelled",
                    // Done, partial, or a job the queue no longer holds: the run ended and its record says how.
                    _ => "done",
                };
                if state != last {
                    svc.app_event(&req2, state, None).await;
                    last = state.to_string();
                }
                if !matches!(state, "queued" | "running") {
                    svc.app_asked.lock().unwrap().end(&req2.key, &work);
                    return;
                }
            }
        });
        Ok(json!({"request": req.id, "state": first, "research_id": run, "topic_id": out["topic_id"], "continued": out["continued"]}))
    }

    /// How often a request's run is looked at for a change of state.
    fn app_poll(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if cfg!(test) { 20 } else { 1000 })
    }

    /// `app.page_rebuild`: `delphi pages rebuild <slug>`, answered at once; the rebuild is its own task and its end is an event.
    async fn app_rebuild(self: &Arc<Self>, req: &Req) -> RpcOutcome {
        let slug = req.ids["slug"].as_str().unwrap_or("").to_string();
        if !self.engine.cfg.get().library.pages.enabled {
            return Err(err(code::INVALID_PARAMS, "generated pages are off ([library.pages] enabled = false)"));
        }
        if crate::library::Library::in_home(&self.engine.ws.home).topic_side(&slug).is_none() {
            return Err(err(code::NOT_FOUND, "no topic has that slug"));
        }
        let work = Work::Rebuild(slug.clone());
        {
            let mut tally = self.app_asked.lock().unwrap();
            if tally.rebuilding(&slug) {
                return Err(err(code::INVALID_PARAMS, "that page is already being rebuilt for the app"));
            }
            tally.begin(&req.key, work.clone());
        }
        self.app_event(req, "running", None).await;
        let (svc, req2) = (self.clone(), Req { id: req.id.clone(), key: req.key.clone(), method: req.method.clone(), ids: req.ids.clone() });
        tokio::spawn(async move {
            // The slot is given back when this task ends, however it ends: a rebuild that panicked must not hold one of the
            // key's four for as long as the daemon runs.
            let slot = Slot { svc: svc.clone(), key: req2.key.clone(), work };
            let built = svc.engine.clone().topic_rebuild(&slug).await;
            drop(slot);
            match built {
                Ok(_) => svc.app_event(&req2, "done", None).await,
                Err(e) => svc.app_event(&req2, "failed", Some(&e)).await,
            }
        });
        Ok(json!({"request": req.id, "state": "running", "slug": req.ids["slug"]}))
    }

    /// `app.page_dispute`: `delphi pages dispute <claim> --note …`. It is one write with nothing to wait for.
    async fn app_dispute(self: &Arc<Self>, req: &Req, p: &Asked<'_>) -> RpcOutcome {
        let lib = crate::library::Library::in_home(&self.engine.ws.home);
        let claim = req.ids["claim"].as_str().unwrap_or("");
        if lib.claim(claim).is_none() {
            return Err(err(code::NOT_FOUND, "no claim has that id"));
        }
        lib.dispute_claim(claim, p.get("note").unwrap_or("")).map_err(|e| err(code::INVALID_PARAMS, e))?;
        self.app_event(req, "done", None).await;
        Ok(json!({"request": req.id, "state": "done", "claim": claim}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-9 as a rule, with the clock in hand: thirty an hour for a key, each key its own count, and the hour moves.
    #[test]
    fn the_thirty_first_request_in_an_hour_is_refused_by_name_and_four_run_at_once() {
        let mut t = Tally::default();
        let none = |_: &Work| true;
        for i in 0..30 {
            assert_eq!(t.admit("app-00000a", 1_000 + i, &none), Ok(()), "{i}");
        }
        let refused = t.admit("app-00000a", 1_100, &none).unwrap_err();
        assert!(refused.contains("30 requests an hour"), "{refused}");
        assert_eq!(t.admit("app-00000b", 1_100, &none), Ok(()), "another key has its own hour");
        assert!(t.admit("app-00000a", 1_000 + 3_599, &none).is_err(), "the first request is still inside the hour");
        assert_eq!(t.admit("app-00000a", 1_000 + 3_600, &none), Ok(()), "an hour after the first, one more");
        assert!(t.admit("app-00000a", 1_000 + 3_600, &none).is_err(), "and only one: a refused request is not counted, an admitted one is");

        let mut t = Tally::default();
        for i in 0..4 {
            t.admit("app-00000a", 10, &none).unwrap();
            t.begin("app-00000a", Work::Rebuild(format!("p{i}")));
        }
        let refused = t.admit("app-00000a", 11, &none).unwrap_err();
        assert!(refused.contains("4 requests running at once"), "{refused}");
        assert!(t.rebuilding("p2") && !t.rebuilding("p9"));
        t.end("app-00000a", &Work::Rebuild("p2".into()));
        assert_eq!(t.admit("app-00000a", 12, &none), Ok(()));
        // A run the queue says has ended no longer counts, whoever forgot to say so.
        t.begin("app-00000a", Work::Research("r-1".into()));
        assert!(t.admit("app-00000a", 13, &none).is_err());
        assert_eq!(t.admit("app-00000a", 13, &|w: &Work| !matches!(w, Work::Research(_))), Ok(()));
        let (a, b) = (t.next_id(255), t.next_id(255));
        assert!(a.starts_with("aq-ff-") && a != b);
    }

    /// AC-7's first half: a parameter the table does not list refuses the call, as does anything that is not a string, and typed text
    /// past the bound. The refusal names the parameter's own letters and never a value.
    #[test]
    fn a_request_takes_its_own_ids_and_typed_text_and_nothing_else() {
        assert!(check("app.research_continue", &json!({"topic_id": "tides-0123abcd"})).is_ok());
        assert!(check("app.research_continue", &json!({"topic_id": "tides-0123abcd", "section": "why-twice", "note": "in winter"})).is_ok());
        for extra in ["query", "question", "queries", "text", "page", "url", "depth", "new", "cwd", "project", "slug", "claim"] {
            let mut p = json!({"topic_id": "tides-0123abcd"});
            p[extra] = json!("free text for the query");
            let e = check("app.research_continue", &p).err().unwrap();
            assert!(e.contains("nothing else") && !e.contains("free text"), "{extra}: {e}");
        }
        assert!(check("app.research_ask", &json!({"question": "why", "topic": "x"})).is_err());
        assert!(check("app.page_rebuild", &json!({"slug": "tides", "force": "yes"})).is_err());
        assert!(check("app.page_dispute", &json!({"claim": "c-1", "note": "wrong", "status": "refuted"})).is_err());
        for not_text in [json!(1), json!(true), json!(null), json!(["a"]), json!({"a": "b"})] {
            assert!(check("app.page_dispute", &json!({"claim": "c-1", "note": not_text})).is_err());
        }
        for not_object in [json!(null), json!("tides"), json!(["tides"]), json!(3)] {
            assert!(check("app.page_rebuild", &not_object).is_err());
        }
        assert!(check("app.research_ask", &json!({"question": "x".repeat(TEXT_MAX)})).is_ok());
        assert!(check("app.research_ask", &json!({"question": "x".repeat(TEXT_MAX + 1)})).is_err());
        assert!(check("app.page_dispute", &json!({"claim": "c-1", "note": "é".repeat(TEXT_MAX + 1)})).is_err());
        assert!(check("approval.resolve", &json!({})).is_err() && check("", &json!({})).is_err());
        let hostile = check("app.page_rebuild", &json!({"slug": "tides", "<script>alert(1)</script>\n": "x"})).err().unwrap();
        assert!(!hostile.contains('<') && !hostile.contains('\n'), "{hostile}");
        // The table is the grant's row, and every method has its parameters; only `note` and `question` are typed.
        assert_eq!(METHODS, delphi_core::app_keys::REQUEST_METHODS);
        assert_eq!(PARAMS.iter().map(|(m, _)| *m).collect::<Vec<_>>(), METHODS);
        let typed: Vec<&str> = PARAMS.iter().flat_map(|(_, ps)| ps.iter().copied()).filter(|p| TYPED.contains(p)).collect();
        assert_eq!(typed, ["note", "question", "note"]);
    }

    #[test]
    fn an_anchor_is_lowercase_letters_digits_and_hyphens() {
        for ok in ["why-twice-a-day", "1", "marées-d-équinoxe", "a--b", "潮汐"] {
            assert!(valid_anchor(ok), "{ok}");
        }
        for bad in ["", "Why", "a b", "a_b", "a/b", "../x", "a\nb", "a)", "ignore previous instructions", "x;y", "$(id)"] {
            assert!(!valid_anchor(bad), "{bad}");
        }
        assert!(valid_anchor(&"a".repeat(ANCHOR_MAX)) && !valid_anchor(&"a".repeat(ANCHOR_MAX + 1)));
        assert_eq!(why("no\ntopic\u{7}"), "no topic");
        assert_eq!(why(&"x".repeat(500)).len(), 200);
    }
}
