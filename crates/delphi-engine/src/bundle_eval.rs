//! The retrieval gate of a bundle (`specs/027` §7.1): `delphi kb eval --bundle <dir>`.
//!
//! The bundle's own `evals/delphi/kb-eval.json` (`{"q": ..., "expect": "knowledge/….md" | null}`) is run against an
//! index that holds only that bundle, as `dev/kb-eval.json` is run against the documents. The thresholds are frozen
//! here and **advisory**: a bundle below them is reported and the numbers are recorded, never blocked.

use crate::kb::{self, BundleTag, EvalCase, EvalReport, Kb, KbError, SearchOpts};
use std::path::Path;

/// Frozen by `specs/027` §7.1. Changing one is a spec amendment, not a tuning knob.
pub const MIN_RECALL_AT_10: f64 = 0.80;
pub const MIN_MRR: f64 = 0.50;
pub const MAX_CONTROLS_ANSWERED: usize = 0;

pub const QUESTIONS: &str = "evals/delphi/kb-eval.json";
pub const RECORD: &str = ".delphi-bundle/eval.json";

/// The numbers the gate looks at, taken from the strongest ranking that could run on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    /// `lexical`, `hybrid` or `+rerank`: what the numbers were measured with.
    pub mode: String,
    pub questions: usize,
    pub controls: usize,
    pub recall10: f64,
    pub mrr: f64,
    pub controls_answered: usize,
}

impl Gate {
    /// What is below its threshold, in words; empty when the bundle passes.
    pub fn shortfalls(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.recall10 < MIN_RECALL_AT_10 {
            v.push(format!("recall@10 {:.2} is below {:.2}", self.recall10, MIN_RECALL_AT_10));
        }
        if self.mrr < MIN_MRR {
            v.push(format!("MRR {:.3} is below {:.2}", self.mrr, MIN_MRR));
        }
        if self.controls_answered > MAX_CONTROLS_ANSWERED {
            v.push(format!("{} of {} off-topic controls were answered (none should be)", self.controls_answered, self.controls));
        }
        v
    }

    pub fn passed(&self) -> bool {
        self.shortfalls().is_empty()
    }

    /// The record written to `.delphi-bundle/eval.json`. The embedder is named so that a reader knows whether the numbers
    /// came from the shipped models (`specs/027` §7.1) or from words alone.
    pub fn record(&self, embedder: Option<&str>, date: &str) -> String {
        let v = serde_json::json!({
            "schema": 1,
            "measured": date,
            "mode": self.mode,
            "embedder": embedder,
            "questions": self.questions,
            "controls": self.controls,
            "recall_at_10": round(self.recall10, 3),
            "mrr": round(self.mrr, 3),
            "controls_answered": self.controls_answered,
            "thresholds": {"recall_at_10": MIN_RECALL_AT_10, "mrr": MIN_MRR, "controls_answered": MAX_CONTROLS_ANSWERED},
            "passed": self.passed(),
            "note": "publisher-reported and advisory; measures findability of the publisher's own questions, not usefulness",
        });
        serde_json::to_string_pretty(&v).expect("a json value serializes") + "\n"
    }
}

fn round(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

/// The gate's numbers from a finished report: the last (strongest) ranking.
pub fn gate(rep: &EvalReport, controls: usize) -> Gate {
    let (mode, r) = rep.per_mode.last().map(|(m, r)| (m.to_string(), *r)).unwrap_or_default();
    let controls_answered = rep.off_topic.last().map(|(_, answered, _)| *answered).unwrap_or(0);
    Gate { mode, questions: rep.cases, controls, recall10: r.at10, mrr: r.mrr, controls_answered }
}

/// `(org, name)` from the bundle's `bundle.toml`.
pub fn identity(dir: &Path) -> Result<(String, String), String> {
    let text = std::fs::read_to_string(dir.join("bundle.toml")).map_err(|e| format!("{}: {}", dir.join("bundle.toml").display(), e))?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| format!("bundle.toml: {}", e))?;
    let get = |k: &str| v.get("bundle").and_then(|b| b.get(k)).and_then(|x| x.as_str()).map(String::from).ok_or(format!("bundle.toml has no [bundle] {}", k));
    let (org, name) = (get("org")?, get("name")?);
    if !delphi_core::bundles::plain_id(&org) || !delphi_core::bundles::plain_id(&name) {
        return Err("bundle.toml: org and name must be plain ids".into());
    }
    Ok((org, name))
}

/// The bundle's questions as the index's cases. `expect` is a path inside the bundle (`knowledge/…`), which becomes
/// the logical path the index holds; `null` marks an off-topic control. Returns the cases and the control count.
pub fn parse_cases(org: &str, name: &str, text: &str) -> Result<(Vec<EvalCase>, usize), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{}: {}", QUESTIONS, e))?;
    let items = v.as_array().ok_or(format!("{} must be a JSON array of questions", QUESTIONS))?;
    let mut cases = Vec::new();
    let mut controls = 0;
    for (i, it) in items.iter().enumerate() {
        let q = it.get("q").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()).ok_or(format!("question {} has no `q`", i + 1))?;
        match it.get("expect") {
            Some(serde_json::Value::String(p)) if !p.is_empty() => {
                let rel = p.trim_start_matches("./");
                if rel.contains("..") || !rel.starts_with("knowledge/") {
                    return Err(format!("question {}: `expect` must be a path under knowledge/ (got `{}`)", i + 1, p));
                }
                cases.push(EvalCase { q: q.to_string(), path: format!("bundles/{}/{}/{}", org, name, rel), heading: it.get("heading").and_then(|h| h.as_str()).unwrap_or("").to_string(), none: false });
            }
            Some(serde_json::Value::Null) | None => {
                controls += 1;
                cases.push(EvalCase { q: q.to_string(), path: String::new(), heading: String::new(), none: true });
            }
            Some(_) => return Err(format!("question {}: `expect` is a path string or null", i + 1)),
        }
    }
    Ok((cases, controls))
}

/// Everything one run of the gate produced.
pub struct Run {
    pub report: EvalReport,
    pub gate: Gate,
    pub embedder: Option<String>,
}

/// Index only the bundle's `knowledge/` and run its questions. With `lexical` no model is opened (the numbers then say
/// `lexical`); otherwise the installed embedder and reranker are used when there are any.
pub fn run(home: &Path, dir: &Path, lexical: bool, progress: &mut dyn FnMut(usize, usize)) -> Result<Run, String> {
    let (org, name) = identity(dir)?;
    let text = std::fs::read_to_string(dir.join(QUESTIONS)).map_err(|e| format!("{}: {}", dir.join(QUESTIONS).display(), e))?;
    let (cases, controls) = parse_cases(&org, &name, &text)?;
    let mut kb = Kb::open_in_memory().map_err(|e| e.to_string())?;
    if !lexical {
        kb::attach_default(home, &mut kb).map_err(|e| e.to_string())?;
    }
    let mut files = Vec::new();
    kb::md_files(&dir.join("knowledge"), &format!("bundles/{}/{}/knowledge", org, name), "bundles", "federated_verified", &mut files);
    if files.is_empty() {
        return Err("the bundle has no Markdown under knowledge/".into());
    }
    let tag = BundleTag { id: format!("{}/{}", org, name), label: format!("{}/{}", org, name), stale: false };
    let files: Vec<_> = files.into_iter().map(|f| kb::SourceFile { bundle: Some(tag.clone()), ..f }).collect();
    kb.sync_lexical(&files, &kb::now()).map_err(|e: KbError| e.to_string())?;
    kb.embed_pending(progress).map_err(|e| e.to_string())?;
    let embedder = kb.embedder().map(|e| e.model_id().to_string());
    let report = kb::evaluate(&kb, &cases, &SearchOpts::default()).map_err(|e| e.to_string())?;
    let gate = gate(&report, controls);
    Ok(Run { report, gate, embedder })
}

/// The lines `kb eval --bundle` and `package` print under the full table.
pub fn verdict(g: &Gate) -> String {
    if g.passed() {
        format!("retrieval gate (advisory): passed with {} ranking: recall@10 {:.2} (>= {:.2}), MRR {:.3} (>= {:.2}), {} control(s) answered", g.mode, g.recall10, MIN_RECALL_AT_10, g.mrr, MIN_MRR, g.controls_answered)
    } else {
        format!("retrieval gate (advisory): BELOW the frozen thresholds with {} ranking: {}. Nothing is blocked; adopters see these numbers when they add the bundle.", g.mode, g.shortfalls().join("; "))
    }
}

/// One line for the add summary, from a bundle's `.delphi-bundle/eval.json` text; `None` when it is absent or unreadable.
pub fn summary_line(record: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(record).ok()?;
    let (r, m) = (v["recall_at_10"].as_f64()?, v["mrr"].as_f64()?);
    let answered = v["controls_answered"].as_u64()?;
    let mode = v["mode"].as_str().unwrap_or("?");
    let verdict = if v["passed"].as_bool()? { "meets" } else { "BELOW" };
    Some(format!("retrieval (publisher-reported, {} ranking): recall@10 {:.2}, MRR {:.2}, {} control(s) answered; {} the advisory thresholds", clean(mode), r, m, answered, verdict))
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '-').take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn bundle(questions: &str) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        put(t.path(), "bundle.toml", "[bundle]\nname = \"plat\"\norg = \"acme\"\n");
        put(t.path(), "knowledge/deploy.md", "# Deploy\n\n## Rolling deploy\n\nShip a service to every cluster with the rolling deploy runbook and the canary step.\n");
        put(t.path(), "knowledge/billing.md", "# Billing\n\n## Database choice\n\nBilling uses Postgres rather than Dynamo because of transactions and reporting.\n");
        put(t.path(), QUESTIONS, questions);
        t
    }

    #[test]
    fn a_bundle_whose_questions_are_found_and_whose_controls_are_not_passes_the_gate() {
        let t = bundle(r#"[{"q":"rolling deploy runbook canary","expect":"knowledge/deploy.md"},{"q":"postgres transactions reporting","expect":"knowledge/billing.md"},{"q":"qwertyuiop zxcvbnm","expect":null}]"#);
        let home = tempfile::tempdir().unwrap();
        let r = run(home.path(), t.path(), true, &mut |_, _| {}).unwrap();
        assert_eq!(r.gate.mode, "lexical");
        assert_eq!((r.gate.questions, r.gate.controls, r.gate.controls_answered), (2, 1, 0), "{:?}", r.gate);
        assert!(r.gate.passed(), "{:?}", r.gate);
        assert!(r.embedder.is_none());
    }

    #[test]
    fn a_question_answered_by_the_wrong_file_fails_recall_and_the_record_says_so() {
        let t = bundle(r#"[{"q":"rolling deploy runbook canary","expect":"knowledge/billing.md"},{"q":"postgres transactions","expect":"knowledge/deploy.md"}]"#);
        let home = tempfile::tempdir().unwrap();
        let r = run(home.path(), t.path(), true, &mut |_, _| {}).unwrap();
        assert!(!r.gate.passed());
        assert!(r.gate.shortfalls().iter().any(|s| s.contains("recall@10")), "{:?}", r.gate.shortfalls());
        assert!(verdict(&r.gate).contains("BELOW") && verdict(&r.gate).contains("Nothing is blocked"));
        let rec = r.gate.record(None, "2026-10-26");
        assert!(summary_line(&rec).unwrap().contains("BELOW"));
    }

    #[test]
    fn the_thresholds_are_the_frozen_ones_and_each_one_fails_alone() {
        let ok = Gate { mode: "hybrid".into(), questions: 10, controls: 2, recall10: 0.80, mrr: 0.50, controls_answered: 0 };
        assert!(ok.passed(), "the boundary values pass: >= 0.80, >= 0.50, 0 controls");
        assert_eq!((MIN_RECALL_AT_10, MIN_MRR, MAX_CONTROLS_ANSWERED), (0.80, 0.50, 0));
        assert_eq!(Gate { recall10: 0.79, ..ok.clone() }.shortfalls().len(), 1);
        assert_eq!(Gate { mrr: 0.49, ..ok.clone() }.shortfalls().len(), 1);
        assert_eq!(Gate { controls_answered: 1, ..ok.clone() }.shortfalls().len(), 1);
        let rec: serde_json::Value = serde_json::from_str(&ok.record(Some("bge-base-en-v1.5"), "2026-10-26")).unwrap();
        assert_eq!((rec["passed"].as_bool(), rec["embedder"].as_str(), rec["schema"].as_u64()), (Some(true), Some("bge-base-en-v1.5"), Some(1)));
    }

    #[test]
    fn question_files_that_name_paths_outside_knowledge_or_have_no_text_are_refused() {
        assert!(parse_cases("a", "b", r#"[{"q":"x","expect":"../secret.md"}]"#).is_err());
        assert!(parse_cases("a", "b", r#"[{"q":"x","expect":"README.md"}]"#).is_err());
        assert!(parse_cases("a", "b", r#"[{"q":" ","expect":null}]"#).is_err());
        assert!(parse_cases("a", "b", r#"{"q":"x"}"#).is_err());
        let (c, controls) = parse_cases("a", "b", r#"[{"q":"x","expect":"knowledge/k.md"},{"q":"y","expect":null}]"#).unwrap();
        assert_eq!((c[0].path.as_str(), controls), ("bundles/a/b/knowledge/k.md", 1));
    }

    #[test]
    fn the_golden_bundle_questions_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/bundles/acme-platform");
        let (org, name) = identity(&dir).unwrap();
        let (cases, controls) = parse_cases(&org, &name, &std::fs::read_to_string(dir.join(QUESTIONS)).unwrap()).unwrap();
        assert!(cases.len() >= 10 && controls >= 2);
    }
}
