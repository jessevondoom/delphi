//! `delphi models calibrate` (`specs/056` §5, §10.9, slice M5): ten seeded-defect review cases, each sent once to a model as a bare
//! carrier call with no Delphi role, governance or skills text, scored against the answer key, and kept per model.
//!
//! The run rules of §10.9: the calls and their cost are printed first, and the calls run only on a `y` or `yes` typed at a terminal
//! (not from a pipe, not inside an agent seat; there is no `--yes`). One call at a time (`--concurrency 2` at most); 12,000 tokens
//! per call, the reply cut at a byte ceiling and the call recorded `capped`, a miss. A cache keyed by carrier, CLI version,
//! endpoint, model, model version, case, the case's bytes and the scorer makes a re-run free. Results stay in the workspace's
//! `local/`, and feed routing only (`local/model-evidence.json`, task kind `review`), never a prompt (§10.4).

use crate::train_defects::{self, Difficulty, Key as DefectKey};
use delphi_engine::assign::{record_evidence, EvidenceRow, Tokens};
use delphi_engine::calibration::{self, CaseResult, Key, SCORER_VERSION};
use delphi_engine::harness::{Cli, Event, Launch, Opts, Session, Tools};
use delphi_adapter::config::{AccessPath, Config, CostClass};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

/// The calibration seeds, fixed here (§10.9 rule 3). They must stay disjoint from M7's benchmark seeds (`dev/train/m7/`).
pub const SEEDS: [u64; 10] = [56051, 56052, 56053, 56054, 56055, 56056, 56057, 56058, 56059, 56060];
/// The M7 benchmark's review seeds, reserved (§10.6): calibration never uses them.
#[cfg(test)]
const M7_SEEDS: [u64; 4] = [56071, 56072, 56073, 56074];
/// §10.9 rule 5: tokens per call, and a reply ceiling (about 2,000 tokens) for carriers that offer no output cap.
pub const TOKEN_CAP: u64 = 12_000;
const REPLY_MAX_BYTES: usize = 8_000;
const REPLY_TOKENS: u64 = 2_000;
/// What a calibration call is told besides the case: nothing of a Delphi role (§10.9 rule 3).
const BARE_SYSTEM: &str = "Answer the request below.";
/// The `review_findings` defaults, the bar for an accepted evidence row.
const MIN_RECALL: f64 = 0.5;
const MAX_FALSE_POSITIVES: u64 = 2;
const USAGE: &str = "usage: delphi models calibrate (<endpoint>/<model> | --all-at low|standard|premium) [--concurrency 1|2] [--dry-run]";

pub(crate) struct Case {
    id: String,
    prompt: String,
    key: DefectKey,
    hash: String,
}

pub(crate) fn cases() -> Vec<Case> {
    let (k, decoys) = Difficulty::Medium.defaults();
    SEEDS
        .iter()
        .map(|seed| {
            let art = train_defects::generate_at(*seed, k, decoys, Difficulty::Medium);
            let prompt = train_defects::inline_prompt(&art.source, Difficulty::Medium);
            let bytes = format!("{}\n{}", prompt, art.key.to_json(*seed, "review.py"));
            let hash = Sha256::digest(bytes.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
            Case { id: format!("review-medium-{seed}"), prompt, key: art.key, hash }
        })
        .collect()
}

/// One `(endpoint, model)` to calibrate, with what the cache key and the cost line need.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    endpoint: String,
    model: String,
    cli: Cli,
    cli_version: String,
    model_version: String,
    env_allow: Vec<String>,
    class: CostClass,
    multiplier: Option<f64>,
}

#[derive(Debug, Default)]
pub(crate) struct Outcome {
    text: String,
    tokens: Tokens,
    premium_milli: u64,
    served: Option<String>,
    capped: bool,
}

/// Sends one case. The real one starts the harness CLI through the seat launcher; tests use a fake that records calls.
pub(crate) trait Carrier: Sync {
    fn call(&self, t: &Target, prompt: &str) -> Result<Outcome, String>;
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The pool's harness pairs the arguments select (§10.9 rule 2: `--all-at standard` is the 1x-and-below run).
fn targets(ws_dir: &Path, cfg: &Config, args: &[String], version: &dyn Fn(&str) -> Option<String>) -> Result<(Vec<Target>, Vec<String>), String> {
    let at = flag(args, "--all-at").map(|s| CostClass::parse(&s).ok_or(format!("--all-at takes low, standard or premium, not `{s}`"))).transpose()?;
    let values = [flag(args, "--all-at"), flag(args, "--concurrency")];
    let one = args.iter().find(|a| !a.starts_with("--") && !values.iter().any(|v| v.as_ref() == Some(*a))).cloned();
    if at.is_some() == one.is_some() {
        return Err(USAGE.into());
    }
    let caps = delphi_engine::capabilities::load(&ws_dir.join("local/endpoint-capabilities.json"));
    let near = delphi_engine::assign::read_near_cap(ws_dir);
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    let mut pairs = cfg.pool_entries();
    pairs.sort();
    pairs.dedup();
    for k in pairs {
        if one.as_ref().is_some_and(|o| o != &format!("{}/{}", k.endpoint_id, k.model_id)) {
            continue;
        }
        let (class, multiplier) = delphi_engine::route::pair_cost(cfg, &k.endpoint_id, &k.model_id);
        let wanted = match at {
            Some(CostClass::Standard) => class <= CostClass::Standard,
            Some(c) => class == c,
            None => true,
        };
        if !wanted {
            continue;
        }
        let Some(ep) = cfg.endpoint(&k.endpoint_id) else { continue };
        let Some(cli) = (ep.path == AccessPath::Harness).then(|| ep.harness.as_deref().and_then(Cli::from_harness)).flatten() else {
            notes.push(format!("{}/{}: skipped, an API endpoint is not calibrated by this command (it bills money, not a seat)", k.endpoint_id, k.model_id));
            continue;
        };
        if near.iter().any(|(e, m)| e == &k.endpoint_id && m == &k.model_id) {
            notes.push(format!("{}/{}: skipped, marked near its cap in local/seat-headroom.json (production seats come first)", k.endpoint_id, k.model_id));
            continue;
        }
        let served = caps.iter().find(|c| c.endpoint_id == k.endpoint_id && c.model_id == k.model_id).and_then(|c| c.served_model.clone());
        let env_allow = cfg.harness.get(cli.program()).map(|p| p.env_allow.clone()).unwrap_or_default();
        out.push(Target { endpoint: k.endpoint_id.clone(), model: k.model_id.clone(), cli, cli_version: version(cli.program()).unwrap_or_else(|| "unknown".into()), model_version: served.unwrap_or_else(|| "unreported".into()), env_allow, class, multiplier });
    }
    if out.is_empty() && notes.is_empty() {
        return Err(match one {
            Some(o) => format!("`{o}` is not a pair in the ratified pool (see `delphi models`)"),
            None => "no pool model is in that cost class".into(),
        });
    }
    Ok((out, notes))
}

fn key(t: &Target, c: &Case) -> Key {
    Key { carrier: t.cli.program().into(), cli_version: t.cli_version.clone(), endpoint: t.endpoint.clone(), model: t.model.clone(), model_version: t.model_version.clone(), case_id: c.id.clone(), case_hash: c.hash.clone(), scorer: SCORER_VERSION.into() }
}

/// The command, with the carrier, the operator's answer and the CLI version reader passed in (tests answer them without a terminal).
pub(crate) fn run_with(ws_dir: &Path, cfg: &Config, args: &[String], carrier: &dyn Carrier, yes: &dyn Fn() -> bool, version: &dyn Fn(&str) -> Option<String>, out: &mut (dyn Write + Send)) -> i32 {
    if args.iter().any(|a| a == "--yes") {
        let _ = writeln!(out, "delphi models calibrate: there is no --yes; the calls run only on your yes typed at a terminal");
        return 2;
    }
    let concurrency = match flag(args, "--concurrency").map(|v| v.parse::<usize>()) {
        None => 1,
        Some(Ok(n @ 1..=2)) => n,
        Some(_) => {
            let _ = writeln!(out, "delphi models calibrate: --concurrency is 1 or 2 (production seats are never starved)");
            return 2;
        }
    };
    let (targets, notes) = match targets(ws_dir, cfg, args, version) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "delphi models calibrate: {e}");
            return 2;
        }
    };
    let cases = cases();
    let mut jobs: Vec<(usize, usize)> = Vec::new();
    let (mut tokens, mut premium) = (0u64, 0f64);
    let _ = writeln!(out, "Calibration: {} seeded-defect review cases (medium, inline), one bare call each with no Delphi role text; {} tokens at most per call, concurrency {}.", cases.len(), TOKEN_CAP, concurrency);
    for (ti, t) in targets.iter().enumerate() {
        let rec = calibration::load(ws_dir, &t.endpoint, &t.model);
        let todo: Vec<usize> = (0..cases.len()).filter(|ci| rec.cached(&key(t, &cases[*ci])).is_none()).collect();
        let per_call: Vec<u64> = todo.iter().map(|ci| (cases[*ci].prompt.len() as u64 / 4 + REPLY_TOKENS).min(TOKEN_CAP)).collect();
        let pr = t.multiplier.map_or(0.0, |m| m * todo.len() as f64);
        let _ = writeln!(out, "  {}/{} [{} {}, {} {}]: {} call(s), {} cached, up to {} tokens{}", t.endpoint, t.model, t.cli.program(), t.cli_version, t.class.as_str(), t.multiplier.map_or("no multiplier".into(), |m| format!("{m}x")), todo.len(), cases.len() - todo.len(), per_call.iter().sum::<u64>(), if t.multiplier.is_some() { format!(", {pr} premium request(s)") } else { String::new() });
        tokens += per_call.iter().sum::<u64>();
        premium += pr;
        jobs.extend(todo.into_iter().map(|ci| (ti, ci)));
    }
    for n in &notes {
        let _ = writeln!(out, "  {n}");
    }
    let _ = writeln!(out, "Total: {} call(s), up to {} tokens, {} premium request(s) (Copilot, from the catalog multipliers).", jobs.len(), tokens, premium);
    if jobs.is_empty() {
        let _ = writeln!(out, "Nothing to call: every case is cached.");
    } else if args.iter().any(|a| a == "--dry-run") {
        let _ = writeln!(out, "Dry run: nothing was called.");
        return 0;
    } else if !yes() {
        let _ = writeln!(out, "Nothing was run.");
        return 2;
    }
    let queue = Mutex::new(jobs.into_iter());
    let writer = Mutex::new((out, 0u32));
    std::thread::scope(|s| {
        for _ in 0..concurrency {
            s.spawn(|| {
                while let Some((ti, ci)) = queue.lock().map(|mut q| q.next()).ok().flatten() {
                let (t, c) = (&targets[ti], &cases[ci]);
                let got = carrier.call(t, &c.prompt);
                let Ok(mut w) = writer.lock() else { break };
                let o = match got {
                    Ok(o) => o,
                    Err(e) => {
                        let _ = writeln!(w.0, "  {}/{} {}: failed, not recorded ({e})", t.endpoint, t.model, c.id);
                        w.1 += 1;
                        continue;
                    }
                };
                let total = o.tokens.input + o.tokens.cache_read + o.tokens.cache_write + o.tokens.output;
                let capped = o.capped || total > TOKEN_CAP;
                let score = train_defects::score(&c.key, &o.text);
                let caught = if capped { 0 } else { score.caught.len() as u32 };
                let accepted = !capped && train_defects::verdict(&score, MIN_RECALL, MAX_FALSE_POSITIVES).is_ok();
                let mut rec = calibration::load(ws_dir, &t.endpoint, &t.model);
                rec.put(CaseResult { key: key(t, c), caught, planted: score.defects as u32, false_positives: score.false_positives as u32, unmatched: score.unmatched as u32, tokens: total, premium_milli: o.premium_milli, capped, served: o.served, at: delphi_engine::pool::system_clock()() });
                let row = EvidenceRow { role: "red-team".into(), kind: "review".into(), endpoint_id: t.endpoint.clone(), model_id: t.model.clone(), accepted, tokens: o.tokens, multiplier: t.multiplier };
                if let Err(e) = calibration::save(ws_dir, &rec).and_then(|_| record_evidence(ws_dir, &[row])) {
                    let _ = writeln!(w.0, "  {}/{} {}: could not save ({e})", t.endpoint, t.model, c.id);
                    w.1 += 1;
                    continue;
                }
                let _ = writeln!(w.0, "  {}/{} {}: caught {} of {}, {} false positive(s), {} tokens{}", t.endpoint, t.model, c.id, caught, score.defects, score.false_positives, total, if capped { ", capped (a miss)" } else { "" });
                }
            });
        }
    });
    let Ok((out, failed)) = writer.into_inner() else { return 1 };
    let known = delphi_engine::models::load(ws_dir);
    let class_of = |e: &str, m: &str| known.iter().find(|c| c.endpoint_id == e && c.model_id == m).map(|c| c.class).unwrap_or_default();
    let _ = write!(out, "{}", calibration::report(&calibration::load_all(ws_dir), &class_of));
    if failed > 0 { 1 } else { 0 }
}

/// The operator's yes at a terminal. A pipe, a script and an agent seat are a no: the run spends the operator's premium requests.
fn confirm() -> bool {
    use std::io::{BufRead, IsTerminal};
    if crate::xenia_terminal::inside_an_agent_seat() {
        println!("Refused inside an agent seat: the operator runs a calibration at a terminal.");
        return false;
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        println!("Not a terminal: type the yes at a terminal to run these calls.");
        return false;
    }
    print!("Run these calls? Type y or yes: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The real carrier: the harness CLI started through the seat launcher with no tools, in a scratch directory it leaves behind.
struct Harness;

impl Carrier for Harness {
    fn call(&self, t: &Target, prompt: &str) -> Result<Outcome, String> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        rt.block_on(harness_call(t, prompt))
    }
}

async fn harness_call(t: &Target, prompt: &str) -> Result<Outcome, String> {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("delphi-calibrate-{}-{}", std::process::id(), CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let l = Launch { cli: t.cli, program: t.cli.program().into(), model: t.model.clone(), system_prompt: BARE_SYSTEM.into(), user_prompt: prompt.into(), cwd: dir.clone(), tools: Tools::None, env: delphi_engine::harness::cli_env_with(&t.env_allow), profile: Default::default(), opts: Opts { max_turns: Some(1), ..Default::default() } };
    let mut s = Session::start(&l).await?;
    let (mut o, mut streamed, mut result, mut err) = (Outcome::default(), String::new(), None, None);
    let add = |o: &Outcome, u: &delphi_engine::harness::Tokens| Tokens { input: o.tokens.input + u.input, cache_read: o.tokens.cache_read + u.cache_read, cache_write: o.tokens.cache_write + u.cache_write, output: o.tokens.output + u.output };
    while let Some(ev) = s.next(Duration::from_secs(300)).await {
        match ev {
            Event::Text(x) => streamed.push_str(&x),
            Event::Usage(u) => o.tokens = add(&o, &u),
            Event::Meter { premium_milli, .. } => o.premium_milli = o.premium_milli.max(premium_milli),
            Event::Served(m) => o.served = Some(m),
            Event::Permission(_) => err = Some("the carrier asked for a tool; a calibration call has none".to_string()),
            Event::Result { text, is_error, usage, .. } => {
                if is_error {
                    err = Some(text.chars().take(160).collect());
                }
                let sum = |t: &Tokens| t.input + t.cache_read + t.cache_write + t.output;
                let reported = Tokens { input: usage.input, cache_read: usage.cache_read, cache_write: usage.cache_write, output: usage.output };
                if sum(&reported) > sum(&o.tokens) {
                    o.tokens = reported;
                }
                result = Some(text);
                s.close_input();
            }
            Event::Exit { .. } => break,
            _ => {}
        }
        let total = o.tokens.input + o.tokens.cache_read + o.tokens.cache_write + o.tokens.output;
        if err.is_some() || streamed.len() > REPLY_MAX_BYTES || total > TOKEN_CAP {
            o.capped = err.is_none();
            break;
        }
    }
    s.kill().await;
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(e) = err {
        return Err(e);
    }
    o.text = result.filter(|r| !r.is_empty()).unwrap_or(streamed);
    let mut end = o.text.len().min(REPLY_MAX_BYTES);
    while !o.text.is_char_boundary(end) {
        end -= 1;
    }
    o.text.truncate(end);
    Ok(o)
}

/// `delphi models calibrate ...` (`args` after `calibrate`, without `--workspace`/`--home`).
pub fn cli(ws_dir: &Path, args: &[String]) -> i32 {
    let live = match std::fs::read_to_string(ws_dir.join("config.toml")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi models calibrate: cannot read config.toml: {e}");
            return 1;
        }
    };
    let cfg = match delphi_engine::boot::effective_config(ws_dir, &live).and_then(|(text, _)| Config::parse(&text).map_err(|e| e.join("; "))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi models calibrate: {e}");
            return 1;
        }
    };
    let version = |p: &str| delphi_engine::capabilities::cli_version_bounded(p, 10);
    run_with(ws_dir, &cfg, args, &Harness, &confirm, &version, &mut std::io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// One Copilot endpoint with a 1x model (fast) and a 15x one (frontier), both in the seed table.
    fn cfg() -> Config {
        Config::parse("[[endpoints]]\nid = \"cp\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"copilot\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\nmodels = { fast = \"claude-sonnet-5.5\", workhorse = \"claude-sonnet-5.5\", frontier = \"claude-opus-5.5\" }\n").unwrap()
    }

    /// Answers every case with its key's findings (or nothing), and counts calls.
    struct Fake {
        calls: AtomicUsize,
        tokens: u64,
        perfect: bool,
    }

    impl Carrier for Fake {
        fn call(&self, _t: &Target, prompt: &str) -> Result<Outcome, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let case = cases().into_iter().find(|c| c.prompt == prompt).expect("the prompt is a case's, unchanged");
            let text = if self.perfect { case.key.defects.iter().map(|d| format!("FINDING {} {}: planted.\n", d.line, d.kind)).collect() } else { "No defects.".to_string() };
            Ok(Outcome { text, tokens: Tokens { input: self.tokens, ..Tokens::default() }, premium_milli: 1000, served: None, capped: false })
        }
    }

    fn fake(tokens: u64, perfect: bool) -> Fake {
        Fake { calls: AtomicUsize::new(0), tokens, perfect }
    }

    fn run(ws: &Path, args: &[&str], carrier: &Fake, yes: &dyn Fn() -> bool) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut out: Vec<u8> = Vec::new();
        let code = run_with(ws, &cfg(), &args, carrier, yes, &|_| Some("1.0.0".into()), &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    /// AC-A5: the cost line is printed before any call, and nothing runs (or is written) without the yes.
    #[test]
    fn the_cost_is_printed_first_and_nothing_runs_without_the_yes() {
        let w = tempfile::tempdir().unwrap();
        let f = fake(3000, true);
        let asked = AtomicUsize::new(0);
        let no = || {
            asked.fetch_add(1, Ordering::SeqCst);
            assert_eq!(f.calls.load(Ordering::SeqCst), 0, "no call before the yes");
            false
        };
        let (code, out) = run(w.path(), &["--all-at", "standard"], &f, &no);
        assert_eq!((code, asked.load(Ordering::SeqCst), f.calls.load(Ordering::SeqCst)), (2, 1, 0), "{out}");
        assert!(out.contains("cp/claude-sonnet-5.5") && out.contains("10 call(s)") && out.contains("Total: 10 call(s)") && out.contains("10 premium request(s)"), "{out}");
        assert!(!out.contains("claude-opus-5.5"), "the 1x run leaves the premium model out: {out}");
        assert!(out.find("Total:").unwrap() < out.find("Nothing was run").unwrap());
        assert!(!w.path().join(calibration::DIR).exists() && !w.path().join(delphi_engine::assign::EVIDENCE_FILE).exists());
        // The premium model needs its own run, and its cost line says so.
        let (_, out) = run(w.path(), &["--all-at", "premium", "--dry-run"], &f, &no);
        assert!(out.contains("claude-opus-5.5") && out.contains("Dry run") && !out.contains("Total: 0 call(s)"), "{out}");
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    /// §10.9 rules 6 and 7: a result file per model, evidence for `review`; a second run makes no call and asks nothing.
    #[test]
    fn a_run_writes_a_record_and_evidence_and_a_second_run_makes_no_calls() {
        let w = tempfile::tempdir().unwrap();
        let f = fake(3000, true);
        let (code, out) = run(w.path(), &["cp/claude-sonnet-5.5", "--concurrency", "2"], &f, &|| true);
        assert_eq!((code, f.calls.load(Ordering::SeqCst)), (0, 10), "{out}");
        let rec = calibration::load(w.path(), "cp", "claude-sonnet-5.5");
        assert_eq!(rec.cases.len(), 10);
        assert!(rec.cases.iter().all(|c| c.caught == c.planted && !c.capped && c.key.cli_version == "1.0.0" && c.key.model_version == "unreported"));
        let ev = delphi_engine::assign::read_evidence(w.path(), "red-team");
        assert_eq!((ev.len(), ev[0].runs, ev[0].accepted), (1, 10, 10));
        assert_eq!(ev[0].cost_tokens, 30_000, "tokens times the 1x multiplier");
        assert!(out.contains("caught 100 of 100 planted") && !out.contains('%'), "{out}");
        let never = || panic!("nothing to spend, nothing to ask");
        let (code, out) = run(w.path(), &["cp/claude-sonnet-5.5"], &f, &never);
        assert_eq!((code, f.calls.load(Ordering::SeqCst)), (0, 10), "{out}");
        assert!(out.contains("0 call(s), 10 cached") && out.contains("every case is cached"), "{out}");
        assert_eq!(delphi_engine::assign::read_evidence(w.path(), "red-team")[0].runs, 10, "a cached case adds no evidence");
    }

    /// §10.9 rule 5: a call over the token cap is recorded capped and counts as a miss.
    #[test]
    fn a_call_over_the_token_cap_is_a_miss() {
        let w = tempfile::tempdir().unwrap();
        let (code, _) = run(w.path(), &["cp/claude-sonnet-5.5"], &fake(TOKEN_CAP + 1, true), &|| true);
        assert_eq!(code, 0);
        let rec = calibration::load(w.path(), "cp", "claude-sonnet-5.5");
        assert!(rec.cases.iter().all(|c| c.capped && c.caught == 0));
        assert_eq!(delphi_engine::assign::read_evidence(w.path(), "red-team")[0].accepted, 0);
    }

    /// AC-A5 and §10.9 rule 4: concurrency above 2 is refused, and so is a `--yes`.
    #[test]
    fn concurrency_above_two_and_a_yes_flag_are_refused() {
        let w = tempfile::tempdir().unwrap();
        let f = fake(10, false);
        for bad in [&["cp/claude-sonnet-5.5", "--concurrency", "3"][..], &["cp/claude-sonnet-5.5", "--yes"], &["cp/nope"], &[]] {
            let (code, out) = run(w.path(), bad, &f, &|| true);
            assert_eq!(code, 2, "{bad:?}: {out}");
        }
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    /// §10.6 and §10.9 rule 3: the calibration seeds are disjoint from the M7 benchmark's, and each case is a bare request.
    #[test]
    fn the_seeds_are_disjoint_from_the_benchmarks_and_the_prompt_is_bare() {
        assert!(SEEDS.iter().all(|s| !M7_SEEDS.contains(s)));
        let bench = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/train/m7/benchmark.toml");
        if let Ok(text) = std::fs::read_to_string(bench) {
            assert!(SEEDS.iter().all(|s| !text.contains(&s.to_string())), "a calibration seed is in the M7 benchmark");
        }
        let cs = cases();
        let mut hashes: Vec<&str> = cs.iter().map(|c| c.hash.as_str()).collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 10);
        for c in &cs {
            assert!(c.prompt.starts_with("Below is a small Python module") && !c.prompt.contains("@project") && !c.prompt.to_lowercase().contains("delphi"), "{}", c.id);
        }
        assert!(!BARE_SYSTEM.to_lowercase().contains("delphi") && !BARE_SYSTEM.contains("role"));
    }
}
