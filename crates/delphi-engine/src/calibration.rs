//! Calibration records (`specs/056` §5, §10.9, slice M5): what `delphi models calibrate` measured of one model on the seeded-defect
//! review cases, one file per model under `local/model-calibration/`, and the class proposals read from them.
//!
//! **Routing only** (§10.4, OD-A2). These records are read by `delphi models` (shown to the operator as counts, never a percentage,
//! rank or score) and summarised into `local/model-evidence.json`, which only routing reads. Nothing here is put in a prompt, a
//! context slice, a skill, a pointer or a tool result; a test scans the source for any other reader. Nothing is committed, bundled
//! or sent in a Xenia record. A proposal is never applied: the operator ratifies one by adding a rule to `model-classes.toml`.

use crate::models::Class;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the records live, in the workspace.
pub const DIR: &str = "local/model-calibration";

/// The scorer's version, part of every cache key: a change in how answers are scored makes new keys.
pub const SCORER_VERSION: &str = "seeded-defects/score-1";

/// What makes a call the same call (§10.9 rule 6). A cached key is not called again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    pub carrier: String,
    pub cli_version: String,
    pub endpoint: String,
    pub model: String,
    /// The catalog's version, else the served-model string the carrier last reported for this pair, else `unreported`.
    pub model_version: String,
    pub case_id: String,
    pub case_hash: String,
    pub scorer: String,
}

/// One case's result (§10.9 rule 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResult {
    pub key: Key,
    pub caught: u32,
    pub planted: u32,
    pub false_positives: u32,
    pub unmatched: u32,
    /// Input + cache read + cache write + output (§10.3).
    pub tokens: u64,
    /// Copilot's own meter, in thousandths of a premium request (0 elsewhere).
    pub premium_milli: u64,
    /// Stopped at the token cap or the reply ceiling: counted as a miss (nothing caught).
    pub capped: bool,
    /// The model the carrier said it served, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served: Option<String>,
    pub at: i64,
}

/// One model's record.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Record {
    pub endpoint: String,
    pub model: String,
    pub cases: Vec<CaseResult>,
}

impl Record {
    /// The result cached for exactly this key.
    pub fn cached(&self, key: &Key) -> Option<&CaseResult> {
        self.cases.iter().find(|c| &c.key == key)
    }

    /// Keep `r`, replacing an older result for the same case (a newer CLI or model version supersedes it).
    pub fn put(&mut self, r: CaseResult) {
        self.cases.retain(|c| c.key.case_id != r.key.case_id);
        self.cases.push(r);
        self.cases.sort_by(|a, b| a.key.case_id.cmp(&b.key.case_id));
    }

    /// Totals over the cases, and the noise band: one standard error of the total caught across the cases (OD-A3).
    pub fn summary(&self) -> Summary {
        let n = self.cases.len() as f64;
        let caught: Vec<f64> = self.cases.iter().map(|c| if c.capped { 0.0 } else { c.caught as f64 }).collect();
        let total: f64 = caught.iter().sum();
        let band = if self.cases.len() < 2 {
            0.0
        } else {
            let mean = total / n;
            let var = caught.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / (n - 1.0);
            var.sqrt() * n.sqrt()
        };
        Summary {
            cases: self.cases.len() as u32,
            caught: total as u32,
            planted: self.cases.iter().map(|c| c.planted).sum(),
            false_positives: self.cases.iter().map(|c| c.false_positives).sum(),
            capped: self.cases.iter().filter(|c| c.capped).count() as u32,
            band,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub cases: u32,
    pub caught: u32,
    pub planted: u32,
    pub false_positives: u32,
    pub capped: u32,
    pub band: f64,
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
}

pub fn path(ws_dir: &Path, endpoint: &str, model: &str) -> PathBuf {
    ws_dir.join(DIR).join(safe(endpoint)).join(format!("{}.json", safe(model)))
}

pub fn load(ws_dir: &Path, endpoint: &str, model: &str) -> Record {
    std::fs::read_to_string(path(ws_dir, endpoint, model)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| Record { endpoint: endpoint.into(), model: model.into(), cases: vec![] })
}

pub fn save(ws_dir: &Path, rec: &Record) -> std::io::Result<()> {
    let p = path(ws_dir, &rec.endpoint, &rec.model);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    delphi_core::lock::write_atomic(&p, &serde_json::to_vec_pretty(rec).map_err(std::io::Error::other)?)
}

/// Every record in the workspace.
pub fn load_all(ws_dir: &Path) -> Vec<Record> {
    let mut out = Vec::new();
    for ep in std::fs::read_dir(ws_dir.join(DIR)).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(ep.path()).into_iter().flatten().flatten() {
            if let Some(r) = std::fs::read_to_string(f.path()).ok().and_then(|t| serde_json::from_str::<Record>(&t).ok()) {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| (&a.endpoint, &a.model).cmp(&(&b.endpoint, &b.model)));
    out
}

fn one_up(c: Class) -> Option<Class> {
    match c {
        Class::Quick => Some(Class::Steady),
        Class::Steady => Some(Class::Deep),
        _ => None,
    }
}

fn one_down(c: Class) -> Option<Class> {
    match c {
        Class::Deep => Some(Class::Steady),
        Class::Steady => Some(Class::Quick),
        _ => None,
    }
}

/// The operator's view (§10.4, §10.9 rule 9): each record as counts, then a proposal to move a model one class when its caught count
/// differs from its prior class's median by more than its band, or "no separation". `class_of` is the model's prior class.
pub fn report(records: &[Record], class_of: &dyn Fn(&str, &str) -> Class) -> String {
    if records.is_empty() {
        return String::new();
    }
    let mut s = String::from("calibration (seeded-defect review cases; routing evidence only, never a prompt):\n");
    for r in records {
        let m = r.summary();
        s.push_str(&format!("  {}/{} [{}]: caught {} of {} planted over {} case(s), {} false positive(s){}, band {:.1}\n", r.endpoint, r.model, class_of(&r.endpoint, &r.model).as_str(), m.caught, m.planted, m.cases, m.false_positives, if m.capped > 0 { format!(", {} capped", m.capped) } else { String::new() }, m.band));
    }
    let mut proposals = Vec::new();
    for r in records {
        let class = class_of(&r.endpoint, &r.model);
        let mut peers: Vec<u32> = records.iter().filter(|p| class_of(&p.endpoint, &p.model) == class).map(|p| p.summary().caught).collect();
        if peers.len() < 2 {
            continue;
        }
        peers.sort_unstable();
        let median = if peers.len() % 2 == 1 { peers[peers.len() / 2] as f64 } else { (peers[peers.len() / 2 - 1] + peers[peers.len() / 2]) as f64 / 2.0 };
        let m = r.summary();
        let diff = m.caught as f64 - median;
        let to = if diff > m.band { one_up(class) } else if -diff > m.band { one_down(class) } else { None };
        if let Some(to) = to {
            proposals.push(format!("  proposal: {}/{} from {} to {} (caught {}, {} median {}, band {:.1}). Not applied: ratify it by adding a rule to model-classes.toml\n", r.endpoint, r.model, class.as_str(), to.as_str(), m.caught, class.as_str(), median, m.band));
        }
    }
    if proposals.is_empty() {
        s.push_str("  no separation: every model is inside its band, so nothing is proposed\n");
    } else {
        s.extend(proposals);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(case: &str, caught: u32) -> CaseResult {
        let key = Key { carrier: "copilot".into(), cli_version: "1.0".into(), endpoint: "e".into(), model: "m".into(), model_version: "unreported".into(), case_id: case.into(), case_hash: "h".into(), scorer: SCORER_VERSION.into() };
        CaseResult { key, caught, planted: 10, false_positives: 1, unmatched: 0, tokens: 5000, premium_milli: 1000, capped: false, served: None, at: 0 }
    }

    fn rec(ep: &str, model: &str, caught: &[u32]) -> Record {
        Record { endpoint: ep.into(), model: model.into(), cases: caught.iter().enumerate().map(|(i, c)| result(&format!("c{i}"), *c)).collect() }
    }

    #[test]
    fn a_record_round_trips_and_a_newer_result_replaces_its_case() {
        let w = tempfile::tempdir().unwrap();
        let mut r = load(w.path(), "cp", "x/y");
        r.put(result("c1", 3));
        r.put(result("c1", 5));
        save(w.path(), &r).unwrap();
        assert!(w.path().join("local/model-calibration/cp/x_y.json").is_file());
        let back = load(w.path(), "cp", "x/y");
        assert_eq!(back.cases.len(), 1);
        assert_eq!(back.cases[0].caught, 5);
        assert!(back.cached(&result("c1", 0).key).is_some());
        let mut other = result("c1", 0).key;
        other.model_version = "new".into();
        assert!(back.cached(&other).is_none(), "a new model version makes a new key");
        assert_eq!(load_all(w.path()).len(), 1);
    }

    #[test]
    fn a_capped_case_counts_as_a_miss_and_the_band_is_one_standard_error_of_the_total() {
        let mut r = rec("e", "m", &[2, 4]);
        r.cases[1].capped = true;
        let s = r.summary();
        assert_eq!((s.caught, s.planted, s.capped), (2, 20, 1));
        // per-case 2 and 0: sd = sqrt(2), times sqrt(2) cases = 2.
        assert!((s.band - 2.0).abs() < 1e-9, "{}", s.band);
    }

    #[test]
    fn a_proposal_needs_a_difference_beyond_the_band_and_is_counts_only() {
        let class = |_: &str, _: &str| Class::Steady;
        let flat = [rec("e", "a", &[5, 5, 5, 5]), rec("e", "b", &[5, 5, 5, 5]), rec("e", "c", &[5, 5, 5, 5])];
        let text = report(&flat, &class);
        assert!(text.contains("no separation") && !text.contains("proposal"), "{text}");
        let apart = [rec("e", "a", &[9, 9, 9, 8]), rec("e", "b", &[5, 5, 5, 5]), rec("e", "c", &[1, 1, 1, 2])];
        let text = report(&apart, &class);
        assert!(text.contains("proposal: e/a from steady to deep") && text.contains("proposal: e/c from steady to quick"), "{text}");
        assert!(!text.contains("e/b from"), "{text}");
        assert!(!text.contains('%') && !text.contains("rank") && !text.contains("score"), "counts only: {text}");
        // Alone in its class: nothing to compare with, nothing proposed.
        assert!(report(&apart[..1], &class).contains("no separation"));
    }

    /// §10.4: calibration reaches routing and the operator's view only. Outside this module, the runner (`models_calibrate.rs`) and
    /// `models_cli.rs`, no source names the records' folder, so no prompt, context or skill builder can read them.
    #[test]
    fn nothing_but_routing_and_the_operators_view_reads_the_records() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let allowed = ["calibration.rs", "models_calibrate.rs", "models_cli.rs"];
        let mut stack = vec![root.clone()];
        let mut offenders = Vec::new();
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() && name != "target" && !name.starts_with('.') {
                    stack.push(p);
                } else if name.ends_with(".rs") && !allowed.contains(&name.as_str()) {
                    let text = std::fs::read_to_string(&p).unwrap_or_default();
                    if text.contains("model-calibration") || text.contains("calibration::") {
                        offenders.push(p.display().to_string());
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "calibration records read outside routing: {offenders:?}");
    }
}
