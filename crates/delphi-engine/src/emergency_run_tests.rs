//! The emergency stop-gap through the engine, with a scripted model (`specs/056` §3.5, M6).

use super::*;
use crate::engine::Status;
use crate::model::Step;
use crate::testkit::{events_of, kinds, setup, Fx, CONFIG};
use std::time::Duration;

const EMERGENCY: &str = r#"
[models.local]
fast = "gemma-x"
[models.catalog."gemma-x"]
max_output_tokens = 2000
context_tokens = 8000
[[endpoints]]
id = "ollama"
family = "local"
path = "api"
credential = "env:L"
cost_model = "local"
base_url = "http://localhost:11434"
"#;

fn config(emergency: bool) -> Config {
    let mut text = CONFIG.replace("allowed_endpoints = [\"direct\", \"backup\"]", "allowed_endpoints = [\"direct\", \"backup\", \"ollama\"]");
    text.push_str(EMERGENCY);
    if emergency {
        text.push_str("\n[routing]\nemergency_endpoint = \"ollama\"\nemergency_model = \"gemma-x\"\n");
    }
    Config::parse(&text).unwrap_or_else(|e| panic!("{e:?}"))
}

fn git(repo: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap().status.success();
    assert!(ok, "{args:?}");
}

/// Every ratified Claude seat is out of quota until an hour from now.
fn all_out(f: &Fx) {
    let reset = f.now.load(Ordering::SeqCst) + 3600;
    for ep in ["direct", "backup"] {
        for model in ["opus-x", "sonnet-x", "haiku-x"] {
            f.engine.pool.record_exhausted(ep, model, &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
        }
    }
}

fn spawn(f: &Fx, prompt: &'static str) -> tokio::task::JoinHandle<Result<crate::engine::TaskOutcome, EngineError>> {
    let (e, m) = (f.engine.clone(), f.m.clone());
    tokio::spawn(async move { e.run_task(&m, "architect", prompt).await })
}

async fn wait_for(f: &Fx, kind: &str, n: usize) {
    // Generous: the live test waits on a local model that may have to load first.
    for _ in 0..12_000 {
        if events_of(f, kind).len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no `{kind}` event appeared: {:?}", kinds(f));
}

async fn finished(h: tokio::task::JoinHandle<Result<crate::engine::TaskOutcome, EngineError>>) -> crate::engine::TaskOutcome {
    tokio::time::timeout(Duration::from_secs(300), h).await.expect("the task finishes").unwrap().unwrap()
}

fn patient(s: &mut crate::engine::Settings) {
    s.park_max = None;
}

fn records(f: &Fx) -> Vec<PathBuf> {
    emergency::pending(&f.m.dir)
}

#[tokio::test]
async fn every_seat_out_saves_state_the_task_waits_the_seat_returns_the_record_is_verified_and_only_then_the_task_runs() {
    let f = setup(vec![Step::text("Slice a is half done."), Step::text("Corrected: slice a is half done."), Step::text("Done after the wait.")], 5.0, patient);
    f.engine.cfg.set(config(true));
    let repo = f.root.join("proj");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    all_out(&f);
    let task = spawn(&f, "Plan the change");
    wait_for(&f, "task.blocked", 1).await;

    // Saved: one record, labeled, written by the emergency model alone, with the facts gathered by code.
    let saved = events_of(&f, "emergency.saved");
    assert_eq!(saved.len(), 1, "{:?}", kinds(&f));
    assert_eq!((saved[0].payload["written_by"].as_str(), saved[0].payload["summary_written"].as_bool()), (Some("ollama/gemma-x"), Some(true)));
    let recs = records(&f);
    assert_eq!(recs.len(), 1);
    let rec = emergency::read_record(&recs[0]).unwrap();
    assert_eq!((rec.label.as_deref(), rec.summary.as_str()), (Some(emergency::LABEL), "Slice a is half done."));
    assert_eq!(rec.facts.exhausted_seats.len(), 6);
    assert!(rec.facts.branches.iter().any(|b| b.name == "app:main") && rec.facts.in_flight.iter().any(|i| i.what.contains("Plan the change")) && !rec.facts.journal_pointers.is_empty());
    // The task did not run on the emergency model: its one call was the summary; the task itself waits.
    assert_eq!(f.model.requests().iter().map(|r| r.model_id.as_str()).collect::<Vec<_>>(), vec!["gemma-x"]);
    assert!(!kinds(&f).contains(&"emergency.recovered".to_string()) && !records(&f).is_empty(), "nothing resumes while the label stands");

    // The repository moved on while nothing ran; the seat comes back.
    std::fs::write(repo.join("n.txt"), "new").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "second"]);
    f.advance(7200);
    let out = finished(task).await;
    assert_eq!((out.status, out.text.as_str()), (Status::Done, "Done after the wait."));

    let k = kinds(&f);
    let at = |kind: &str| k.iter().position(|x| x == kind).unwrap_or_else(|| panic!("{kind} in {k:?}"));
    assert!(at("emergency.saved") < at("task.blocked") && at("task.blocked") < at("task.unblocked") && at("task.unblocked") < at("emergency.recovered") && at("emergency.recovered") < at("budget.reserve"), "{k:?}");
    let rec = emergency::read_record(&recs[0]).unwrap();
    assert_eq!((rec.label, rec.verified_by.as_deref(), rec.summary.as_str()), (None, Some("direct/opus-x"), "Corrected: slice a is half done."));
    assert!(rec.corrections.iter().any(|c| c.contains("app:main") && c.contains("head was")), "{:?}", rec.corrections);
    assert!(records(&f).is_empty());
    // The calls, in order: the emergency summary, the verifier (a real seat, told the summary is unverified), then the task.
    let reqs = f.model.requests();
    assert_eq!(reqs.iter().map(|r| r.model_id.as_str()).collect::<Vec<_>>(), vec!["gemma-x", "opus-x", "opus-x"]);
    assert!(reqs[1].body.to_string().contains("unverified"));
    assert_eq!(events_of(&f, "emergency.saved").len(), 1, "once per outage, however often the task looks again");
}

#[tokio::test]
async fn a_failed_or_empty_verifier_keeps_the_label_and_a_pending_record_blocks_a_new_dispatch() {
    let f = setup(vec![Step::text("Draft."), Step::Fail("boom"), Step::text("   "), Step::text("Verified."), Step::text("Ran at last.")], 5.0, patient);
    f.engine.cfg.set(config(true));
    all_out(&f);
    let first = spawn(&f, "Plan the change");
    wait_for(&f, "task.blocked", 1).await;
    f.advance(7200);

    // The verifier's call fails: the first task stops with the reason and the label stays on.
    let out = finished(first).await;
    assert!(matches!(out.status, Status::Escalated(ref r) if r.contains("nothing resumes from it")), "{:?}", out.status);
    assert_eq!(events_of(&f, "emergency.blocked").len(), 1);
    assert_eq!(records(&f).len(), 1);
    assert_eq!(f.model.requests().len(), 2, "the summary and the failed verification; the task never ran");

    // A new dispatch meets the same record: an empty verification does not clear it either, and the task still does not run.
    let out = finished(spawn(&f, "Another task")).await;
    assert!(matches!(out.status, Status::Escalated(ref r) if r.contains("returned nothing")), "{:?}", out.status);
    assert_eq!(events_of(&f, "emergency.blocked").len(), 2);
    assert_eq!(records(&f).len(), 1, "still labeled");
    assert_eq!(f.model.requests().len(), 3);
    assert_eq!(emergency::read_record(&records(&f)[0]).unwrap().label.as_deref(), Some(emergency::LABEL));

    // A verifier that answers clears it, and the next dispatch runs.
    let out = finished(spawn(&f, "A third task")).await;
    assert_eq!((out.status, out.text.as_str()), (Status::Done, "Ran at last."));
    assert!(records(&f).is_empty() && events_of(&f, "emergency.recovered").len() == 1);
}

#[tokio::test]
async fn with_the_emergency_model_unset_nothing_changes() {
    // Same outage, same wait, with a stale labeled record on disk: no emergency event, no record, no extra call, no gate.
    let f = setup(vec![Step::text("Done.")], 5.0, patient);
    f.engine.cfg.set(config(false));
    std::fs::create_dir_all(f.m.dir.join(emergency::DIR)).unwrap();
    std::fs::write(f.m.dir.join(emergency::DIR).join("emergency-1.json"), "{}").unwrap();
    all_out(&f);
    let task = spawn(&f, "Plan the change");
    wait_for(&f, "task.blocked", 1).await;
    let before = kinds(&f);
    assert!(!before.iter().any(|k| k.starts_with("emergency.")), "{before:?}");
    assert_eq!(before.iter().filter(|k| *k == "checkpoint.saved").count(), 1, "the ordinary save-point is unchanged");
    assert!(f.model.requests().is_empty(), "no model was called while waiting");
    assert_eq!(std::fs::read_dir(f.m.dir.join(emergency::DIR)).unwrap().count(), 1, "nothing was written");
    f.advance(7200);
    let out = finished(task).await;
    assert_eq!((out.status, out.text.as_str()), (Status::Done, "Done."));
    assert!(!kinds(&f).iter().any(|k| k.starts_with("emergency.")));
    assert_eq!(f.model.requests().len(), 1, "the only call was the task's own");
}

#[tokio::test]
async fn one_seat_out_is_a_failover_not_a_stop() {
    let f = setup(vec![Step::text("Done on the other seat.")], 5.0, patient);
    f.engine.cfg.set(config(true));
    f.engine.pool.record_exhausted("direct", "opus-x", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(f.now.load(Ordering::SeqCst) + 3600), raw: String::new() });
    let out = finished(spawn(&f, "Plan the change")).await;
    assert_eq!(out.status, Status::Done);
    assert!(!kinds(&f).iter().any(|k| k.starts_with("emergency.")) && records(&f).is_empty());
    assert!(f.model.requests().iter().all(|r| r.model_id != "gemma-x"), "the emergency model never serves a task");
}

#[tokio::test]
async fn the_writer_refuses_anything_the_stop_gap_may_not_do() {
    let f = setup(vec![], 5.0, patient);
    let key = PoolKey { endpoint_id: "ollama".into(), model_id: "gemma-x".into(), tier: "fast".into(), family: "local".into() };
    for ok in ["read", "summarise", "write-state"] {
        assert!(ModelWriter::new(&f.engine, key.clone(), ok, false).is_ok(), "{ok}");
    }
    for bad in ["approve", "ratify", "merge", "spend", "push", "shell", "network", "dispatch"] {
        assert!(ModelWriter::new(&f.engine, key.clone(), bad, false).err().unwrap().contains("may only read"), "{bad}");
    }
}

/// The whole path against real local models (Ollama: `gemma4` is the emergency model, `mistral` the only ratified seat). Run by hand:
/// `cargo test -p delphi-engine --lib live_emergency -- --ignored --nocapture`. Free; nothing leaves the machine.
#[tokio::test]
#[ignore]
async fn live_emergency_path_on_ollama() {
    let mut f = setup(vec![], 5.0, patient);
    let text = r#"
[models.local2]
fast = "mistral:latest"
workhorse = "mistral:latest"
frontier = "mistral:latest"
[models.local]
fast = "gemma4:latest"
[models.catalog."mistral:latest"]
max_output_tokens = 2048
context_tokens = 8000
[models.catalog."gemma4:latest"]
max_output_tokens = 2048
context_tokens = 8000
[[endpoints]]
id = "mistral"
family = "local2"
path = "gateway-compat"
base_url = "http://localhost:11434"
credential = "none"
cost_model = "local"
[[endpoints]]
id = "gemma"
family = "local"
path = "gateway-compat"
base_url = "http://localhost:11434"
credential = "none"
cost_model = "local"
[access]
order = { local2 = ["mistral"], local = ["gemma"] }
allowed_endpoints = ["mistral", "gemma"]
[routing]
emergency_endpoint = "gemma"
emergency_model = "gemma4:latest"
"#;
    f.engine.cfg.set(Config::parse(text).unwrap_or_else(|e| panic!("{e:?}")));
    let client = crate::boot::http_model_client(f.engine.cfg.clone(), f.engine.secrets.clone());
    std::sync::Arc::get_mut(&mut f.engine).expect("no other holder of the engine").model = client;
    let reset = f.now.load(Ordering::SeqCst) + 3600;
    f.engine.pool.record_exhausted("mistral", "mistral:latest", &delphi_adapter::exhaust::Exhaustion { reason: "quota reached".into(), reset_at: Some(reset), raw: String::new() });
    // A scout's floor is `quick`, which a local model meets (an architect's is `steady`, which it does not, so it could not verify).
    let (e, m) = (f.engine.clone(), f.m.clone());
    let task = tokio::spawn(async move { e.run_task(&m, "scout", "Reply with the single word OK.").await });
    wait_for(&f, "emergency.saved", 1).await;
    let rec = emergency::read_record(&records(&f)[0]).unwrap();
    println!("SAVED label={:?} by={} summary={:?}", rec.label, rec.written_by, rec.summary);
    f.advance(7200);
    let out = finished(task).await;
    println!("OUTCOME {:?} {:?}", out.status, out.text);
    println!("EVENTS {:?}", kinds(&f));
    for kind in ["emergency.saved", "emergency.recovered", "emergency.blocked"] {
        for e in events_of(&f, kind) {
            println!("{kind} {}", e.payload);
        }
    }
    println!("RECORD {}", std::fs::read_to_string(std::fs::read_dir(f.m.dir.join(emergency::DIR)).unwrap().next().unwrap().unwrap().path()).unwrap());
}

#[test]
fn the_log_gives_pointers_and_the_work_in_flight_without_a_model() {
    let ev = |seq: u64, kind: &str, task: &str, payload: serde_json::Value| {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.seq = seq;
        e.task_id = Some(task.into());
        e
    };
    let log = vec![ev(1, "task.created", "t1", json!({"prompt": "done one"})), ev(2, "task.completed", "t1", json!({})), ev(3, "task.created", "t2", json!({"prompt": "open  one"})), ev(4, "agent.usage", "t2", json!({}))];
    let (pointers, flight) = log_facts(&log);
    assert_eq!(flight.len(), 1);
    assert_eq!(flight[0].what, "t2: open one");
    assert!(pointers.contains(&"seq:3".to_string()) && pointers.contains(&"seq:4".to_string()));
    assert!(journal_ids(&log).contains(&"seq:1".to_string()));
}
