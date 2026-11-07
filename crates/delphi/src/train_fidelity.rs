//! `delphi train fidelity`: what a relay loses between seats (`specs/048` BM-5, "paraphrase loss").
//!
//! A seat reads an artifact that carries N exact items (a hash, a path, a number, a line of code, a quoted sentence) and reports
//! to the Orchestrator, which reports to the operator. The score is the share of items present **byte for byte** in the seat's
//! report (the relayed text) and in the operator's answer (the final text). Two things live here:
//!
//! - `run`: a **scripted chain**. The seat and the Orchestrator are deterministic functions, not models, so the run costs
//!   nothing and proves the instrument discriminates (`specs/048` AC-3): a relay that paraphrases scores below 1.0, a verbatim
//!   relay scores 1.0, and a relay that passes a handle to the raw output (`specs/049` CX-6, modelled here, not built) keeps
//!   every item in the final answer even though the seat's own summary is lossy.
//! - `gen`: writes a **case** a real model can be run on with `delphi train run`: a fixture with the artifact, a prompt that
//!   asks for the items exactly, and an `exact_items` check against a list the model never sees.
//!
//! The scripted paraphrase is a model of what a summarising seat does, not a measurement of any real model.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::train_items::{self as items, Item, ItemKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Relay {
    /// Each stage repeats what it was given.
    Verbatim,
    /// Each stage rewrites items the way a summary does: shortens a hash, keeps a file name, rounds a number.
    Paraphrase,
    /// The seat summarises and hands over a handle to its raw output; the Orchestrator quotes the raw output by handle.
    Handle,
}

impl Relay {
    pub const ALL: [Relay; 3] = [Relay::Verbatim, Relay::Paraphrase, Relay::Handle];

    pub fn name(self) -> &'static str {
        match self {
            Relay::Verbatim => "verbatim",
            Relay::Paraphrase => "paraphrase",
            Relay::Handle => "handle",
        }
    }
}

/// One item as a summary would state it: close in meaning, not byte for byte.
fn lossy(it: &Item) -> String {
    match it.kind {
        ItemKind::Hash => format!("{}…", &it.text[..7.min(it.text.len())]),
        ItemKind::Path => it.text.rsplit('/').next().unwrap_or(&it.text).to_string(),
        ItemKind::Number => match it.text.parse::<f64>() {
            Ok(n) => format!("about {} thousand", (n / 1000.0).round() as i64),
            Err(_) => "a large number".into(),
        },
        // The comment is dropped, as a paraphrase of a line of code drops it.
        ItemKind::CodeLine => it.text.split("//").next().unwrap_or(&it.text).trim().trim_end_matches(';').to_string(),
        // Lower case and no full stop: the same words in a reported form.
        ItemKind::Sentence => {
            let mut c = it.text.trim_end_matches('.').chars();
            c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn paraphrase(text: &str, its: &[Item]) -> String {
    its.iter().fold(text.to_string(), |t, it| t.replace(&it.text, &lossy(it)))
}

/// What each stage said: the seat's report to the Orchestrator, and the Orchestrator's answer to the operator.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub relayed: String,
    pub finalized: String,
}

const HANDLE: &str = "@payload/seat-1";

/// Run the artifact through a seat and an Orchestrator.
pub fn run_chain(artifact: &str, its: &[Item], relay: Relay) -> Chain {
    match relay {
        Relay::Verbatim => Chain { relayed: artifact.to_string(), finalized: artifact.to_string() },
        Relay::Paraphrase => {
            let relayed = paraphrase(artifact, its);
            // The Orchestrator summarises the summary: nothing that was lost comes back.
            let finalized = paraphrase(&relayed, its);
            Chain { relayed, finalized }
        }
        Relay::Handle => {
            let relayed = format!("{}\n\n[raw output kept at {}]\n", paraphrase(artifact, its), HANDLE);
            // The Orchestrator sees the handle, reads it, and quotes the raw output where the operator needs it (CX-6, OD-3).
            let finalized = format!("{}\nFrom the seat's raw output ({}):\n\n{}", relayed, HANDLE, artifact);
            Chain { relayed, finalized }
        }
    }
}

/// One relay's score.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub relay: Relay,
    /// Per kind: items, kept in the seat's report, kept in the operator's answer.
    pub kinds: BTreeMap<ItemKind, (usize, usize, usize)>,
    pub relayed: f64,
    pub finalized: f64,
}

pub fn score(its: &[Item], relay: Relay, chain: &Chain) -> Outcome {
    let (r, f) = (items::survival(its, &chain.relayed), items::survival(its, &chain.finalized));
    let (rk, fk) = (items::by_kind(its, &r), items::by_kind(its, &f));
    Outcome {
        relay,
        kinds: rk.iter().map(|(k, (n, kept))| (*k, (*n, *kept, fk.get(k).map_or(0, |x| x.1)))).collect(),
        relayed: items::share(&r).unwrap_or(0.0),
        finalized: items::share(&f).unwrap_or(0.0),
    }
}

/// The chart-ready table (`specs/048` §5): survival by relay and kind.
pub fn table(seed: u64, n: usize, outcomes: &[Outcome]) -> String {
    let mut out = format!("HANDOFF FIDELITY (seed {}, {} items): items present byte for byte\n", seed, n);
    out.push_str(&format!("{:<12}{:<11}{:>7}{:>10}{:>13}\n", "relay", "kind", "items", "at seat", "at operator"));
    for o in outcomes {
        for (k, (total, r, f)) in &o.kinds {
            out.push_str(&format!("{:<12}{:<11}{:>7}{:>10}{:>13}\n", o.relay.name(), k.name(), total, format!("{}/{}", r, total), format!("{}/{}", f, total)));
        }
        out.push_str(&format!("{:<12}{:<11}{:>7}{:>10}{:>13}\n", o.relay.name(), "ALL", n, format!("{:.0}%", o.relayed * 100.0), format!("{:.0}%", o.finalized * 100.0)));
    }
    out.push_str("A scripted chain: the seat and the Orchestrator are functions, not models. It shows what the instrument can tell apart, not what any model does.\n");
    out
}

pub fn to_json(seed: u64, n: usize, outcomes: &[Outcome]) -> serde_json::Value {
    json!({"seed": seed, "items": n, "relays": outcomes.iter().map(|o| json!({
        "relay": o.relay.name(), "at_seat": o.relayed, "at_operator": o.finalized,
        "kinds": o.kinds.iter().map(|(k, (t, r, f))| (k.name().to_string(), json!({"items": t, "at_seat": r, "at_operator": f}))).collect::<serde_json::Map<_, _>>()})).collect::<Vec<_>>()})
}

// ── a case for a real model ─────────────────────────────────────────────────────────────────────────────────────

/// The prompt: it names no item, and asks for exactness.
const PROMPT: &str = "Ask the investigator to read artifact.md in the project and report back to you. Then give me every identifier, hash, file path, number, line of code and quoted sentence it contains, character for character, as a list. Do not paraphrase, shorten or round anything.";

/// Write a case directory (`case.toml`, `fixture/artifact.md`, `items.json`) for `delphi train run`. `items.json` sits outside
/// the fixture, so the model under test never sees the answer key.
pub fn write_case(dir: &Path, seed: u64, n: usize, split: &str) -> Result<(), String> {
    let its = items::generate(seed, n);
    let id = dir.file_name().and_then(|s| s.to_str()).ok_or("the case directory has no name")?;
    fs::create_dir_all(dir.join("fixture")).map_err(|e| format!("{}: {}", dir.display(), e))?;
    fs::write(dir.join("fixture/artifact.md"), items::artifact(&its)).map_err(|e| e.to_string())?;
    fs::write(dir.join("items.json"), items::items_json(&its)).map_err(|e| e.to_string())?;
    let toml = format!(
        "id = \"{id}\"\nsuite = \"fidelity\"\nkind = \"task\"\nrole = \"orchestrator\"\nsplit = \"{split}\"\ntags = [\"handoff\"]\nmax_cost_usd = 1.0\ntimeout_secs = 900\nprompt = \"{prompt}\"\n\n# The final answer keeps every item byte for byte (seed {seed}, {n} items; `delphi train fidelity gen` made this case).\n[[check]]\nkind = \"exact_items\"\npath = \"items.json\"\nmin = 1.0\n",
        id = id, split = split, prompt = PROMPT, seed = seed, n = n
    );
    fs::write(dir.join("case.toml"), toml).map_err(|e| e.to_string())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

const USAGE: &str = "delphi train fidelity run [--seed <n>] [--items <n>] [--relay verbatim|paraphrase|handle|all] [--json]\n  delphi train fidelity gen [--seed <n>] [--items <n>] [--split dev|holdout] --out <new case dir>\n  run scores a scripted chain (no model, no cost). gen writes a case for `delphi train run` (suite `fidelity`).";

/// `delphi train fidelity run|gen ...` (`args[0]` is `fidelity`).
pub fn cli(args: &[String], suites: Option<&Path>) -> i32 {
    let sub = args.get(1).map(String::as_str);
    let num = |name: &str, default: u64| -> Result<u64, String> { flag(args, name).map_or(Ok(default), |v| v.parse::<u64>().map_err(|_| format!("{} must be a whole number", name))) };
    let (seed, n) = match (num("--seed", 1), num("--items", 10)) {
        (Ok(s), Ok(n)) if (5..=200).contains(&n) => (s, n as usize),
        (Ok(_), Ok(_)) => {
            eprintln!("delphi train fidelity: --items must be between 5 (so every kind appears) and 200");
            return 2;
        }
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("delphi train fidelity: {}", e);
            return 2;
        }
    };
    match sub {
        Some("run") => {
            let relays: Vec<Relay> = match flag(args, "--relay").as_deref() {
                None | Some("all") => Relay::ALL.to_vec(),
                Some(name) => match Relay::ALL.iter().find(|r| r.name() == name) {
                    Some(r) => vec![*r],
                    None => {
                        eprintln!("delphi train fidelity: --relay must be verbatim, paraphrase, handle or all\n  {}", USAGE);
                        return 2;
                    }
                },
            };
            let its = items::generate(seed, n);
            let doc = items::artifact(&its);
            let outcomes: Vec<Outcome> = relays.iter().map(|r| score(&its, *r, &run_chain(&doc, &its, *r))).collect();
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&to_json(seed, n, &outcomes)).unwrap_or_default());
            } else {
                print!("{}", table(seed, n, &outcomes));
            }
            0
        }
        Some("gen") => {
            let split = flag(args, "--split").unwrap_or_else(|| "dev".into());
            if !["dev", "holdout"].contains(&split.as_str()) {
                eprintln!("delphi train fidelity: --split must be dev or holdout");
                return 2;
            }
            let Some(out) = flag(args, "--out").map(PathBuf::from) else {
                eprintln!("delphi train fidelity gen needs --out <new case dir>, such as {}/fidelity/cases/fid-{}-{}\n  {}", suites.map_or("dev/train/suites".into(), |s| s.display().to_string()), seed, n, USAGE);
                return 2;
            };
            if out.exists() {
                eprintln!("delphi train fidelity: {} already exists; give a new directory", out.display());
                return 2;
            }
            match write_case(&out, seed, n, &split) {
                Ok(()) => {
                    println!("wrote {}: {} items (seed {}); `delphi train validate` checks it, `delphi train run` runs it on an arm", out.display(), n, seed);
                    0
                }
                Err(e) => {
                    eprintln!("delphi train fidelity: {}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(relay: Relay) -> Outcome {
        let its = items::generate(4, 10);
        score(&its, relay, &run_chain(&items::artifact(&its), &its, relay))
    }

    #[test]
    fn the_instrument_tells_a_paraphrasing_chain_from_a_verbatim_one() {
        // AC-3: a chain that paraphrases drops items and scores below 1.0; the same chain with verbatim relay scores 1.0.
        let (v, p) = (outcome(Relay::Verbatim), outcome(Relay::Paraphrase));
        assert_eq!((v.relayed, v.finalized), (1.0, 1.0));
        assert_eq!((p.relayed, p.finalized), (0.0, 0.0), "every lossy form differs from its item");
        for (k, (total, r, f)) in &p.kinds {
            assert_eq!((*total, *r, *f), (2, 0, 0), "{:?}: each kind is lost", k);
        }
        assert_eq!(v.kinds.len(), 5);
    }

    #[test]
    fn a_handle_relay_keeps_every_item_in_the_final_answer_though_the_seats_summary_is_lossy() {
        // `specs/049` §5: with CX-6 on, a scripted chain passes BM-5 at 1.0.
        let h = outcome(Relay::Handle);
        assert_eq!((h.relayed, h.finalized), (0.0, 1.0));
        let its = items::generate(4, 10);
        let chain = run_chain(&items::artifact(&its), &its, Relay::Handle);
        assert!(chain.relayed.contains(HANDLE) && !chain.relayed.contains(&its[0].text), "the seat's text holds the handle and not the raw item");
    }

    #[test]
    fn a_lossy_form_is_never_the_item_and_a_partial_paraphrase_scores_in_between() {
        let its = items::generate(9, 10);
        assert!(its.iter().all(|i| lossy(i) != i.text && !lossy(i).contains(&i.text)));
        // A relay that rewrites only the hashes and numbers keeps the other three kinds.
        let doc = items::artifact(&its);
        let part = its.iter().filter(|i| matches!(i.kind, ItemKind::Hash | ItemKind::Number)).fold(doc, |t, i| t.replace(&i.text, &lossy(i)));
        let kept = items::survival(&its, &part);
        assert_eq!(items::share(&kept), Some(0.6));
        assert_eq!(items::by_kind(&its, &kept)[&ItemKind::Path], (2, 2));
    }

    #[test]
    fn the_table_and_json_carry_every_relay_and_kind() {
        let outcomes: Vec<Outcome> = Relay::ALL.iter().map(|r| outcome(*r)).collect();
        let t = table(4, 10, &outcomes);
        for want in ["HANDOFF FIDELITY (seed 4, 10 items)", "paraphrase", "code line", "0/2", "2/2", "ALL", "100%", "A scripted chain"] {
            assert!(t.contains(want), "{} in\n{}", want, t);
        }
        let j = to_json(4, 10, &outcomes);
        assert_eq!((j["relays"][1]["relay"].as_str(), j["relays"][1]["at_operator"].as_f64(), j["relays"][2]["at_operator"].as_f64()), (Some("paraphrase"), Some(0.0), Some(1.0)));
        assert_eq!(j["relays"][0]["kinds"]["hash"]["at_seat"].as_u64(), Some(2));
    }

    #[test]
    fn an_exact_items_check_needs_a_file_inside_the_case_and_a_share_above_zero() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/fidelity/cases/fid-2-5");
        write_case(&dir, 2, 5, "holdout").unwrap();
        assert_eq!(crate::train::load_case(&dir, "fidelity").unwrap().split, crate::train::Split::Holdout);
        let good = fs::read_to_string(dir.join("case.toml")).unwrap();
        for (bad, want) in [
            (good.replace("path = \"items.json\"\n", ""), "needs `path`"),
            (good.replace("path = \"items.json\"", "path = \"../items.json\""), "must be relative"),
            (good.replace("min = 1.0", "min = 0.0"), "needs `min` above 0"),
            (good.replace("min = 1.0", "min = 1.5"), "needs `min` above 0"),
            (good.replace("min = 1.0\n", ""), "needs `min` above 0"),
        ] {
            fs::write(dir.join("case.toml"), &bad).unwrap();
            let e = crate::train::load_case(&dir, "fidelity").unwrap_err().join("; ");
            assert!(e.contains(want), "{} in {}", want, e);
        }
    }

    #[test]
    fn a_generated_case_loads_keeps_its_answers_out_of_the_fixture_and_the_command_checks_its_arguments() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("suites/fidelity/cases/fid-7-10");
        write_case(&dir, 7, 10, "dev").unwrap();
        let case = crate::train::load_case(&dir, "fidelity").unwrap();
        assert_eq!((case.role.as_str(), case.checks.len()), ("orchestrator", 1));
        assert!(matches!(&case.checks[0], crate::train::Check::ExactItems { file, min } if file == "items.json" && *min == 1.0), "{:?}", case.checks);
        let key = fs::read_to_string(dir.join("items.json")).unwrap();
        let its = items::generate(7, 10);
        assert!(key.contains(&its[0].text));
        for f in fs::read_dir(dir.join("fixture")).unwrap().flatten() {
            assert!(f.file_name() != "items.json", "the answer key is not in the fixture");
        }
        assert!(!case.prompt.as_deref().unwrap().contains(&its[0].text), "the prompt gives nothing away");
        // The command: a fresh directory is required, arguments are checked, and `run` needs no repository.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let out = t.path().join("new-case");
        assert_eq!(cli(&a(&["fidelity", "gen", "--seed", "3", "--items", "10", "--out", out.to_str().unwrap()]), None), 0);
        assert_eq!(cli(&a(&["fidelity", "gen", "--out", out.to_str().unwrap()]), None), 2, "an existing directory is refused");
        assert_eq!(cli(&a(&["fidelity", "gen"]), None), 2);
        assert_eq!(cli(&a(&["fidelity", "run", "--items", "3"]), None), 2, "fewer than five items cannot cover every kind");
        assert_eq!(cli(&a(&["fidelity", "run", "--relay", "psychic"]), None), 2);
        assert_eq!(cli(&a(&["fidelity", "run", "--seed", "x"]), None), 2);
        assert_eq!(cli(&a(&["fidelity", "run", "--relay", "paraphrase"]), None), 0);
        assert_eq!(cli(&a(&["fidelity"]), None), 2);
    }
}
