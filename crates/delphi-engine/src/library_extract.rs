//! Key facts out of an imported summary page, and the operator's attestation of them (`specs/068` §6.5, §6.6).
//!
//! A summary supplies no evidence. A candidate is accepted only when its quote is a verbatim substring of a source the
//! library holds whole; the page's own text never counts. Accepted candidates are `attributed` claims that read *not
//! validated* and carry `origin: page:<id>` as provenance. Only the operator's attestation, never a model, records that one
//! holds, and it is a record of its own so it cannot be mistaken for web corroboration.

use super::claims::AnchorSpec;
use super::claims::ClaimOpts;
use super::*;

/// Candidates one page may have open at a time.
pub const MAX_PER_PAGE: usize = 7;
pub const ATTESTED_LABEL: &str = "validated · attested by operator · internal source";

#[derive(Debug, Clone, Default)]
pub struct Candidate {
    pub claim: String,
    pub source_id: String,
    pub quote: String,
    pub locator: String,
}

#[derive(Debug, Default)]
pub struct Extracted {
    pub accepted: Vec<String>,
    /// Already recorded, so not added again.
    pub existing: Vec<String>,
    /// `(position in the input, why)`.
    pub rejected: Vec<(usize, String)>,
}

fn origin_of(page_id: &str) -> String {
    format!("page:{}", page_id)
}

impl Library {
    /// Claims from this page that no one has decided yet.
    pub fn open_candidates(&self, page_id: &str) -> Vec<Value> {
        let o = origin_of(page_id);
        self.claims().into_iter().filter(|c| c["origin"] == o && c["attestation"].is_null() && c["lifecycle"].is_null()).collect()
    }

    /// The anchor guard. Every candidate is checked against the stored text of its source; none is taken on the page's word.
    pub fn extract_candidates(&self, page_id: &str, cands: &[Candidate]) -> Extracted {
        let mut out = Extracted::default();
        let mut open = self.open_candidates(page_id).len();
        for (i, c) in cands.iter().enumerate() {
            let reject = |out: &mut Extracted, why: &str| out.rejected.push((i, why.to_string()));
            let Some(meta) = self.read_meta(&c.source_id) else {
                reject(&mut out, "no such source in the library");
                continue;
            };
            if !meta["purged"].is_null() {
                reject(&mut out, "that source was purged");
                continue;
            }
            let Ok(text) = std::fs::read_to_string(self.source_dir(&c.source_id).join("source.md")) else {
                reject(&mut out, "the source is not held whole, so the quote cannot be checked");
                continue;
            };
            if c.quote.trim().is_empty() || !text.contains(c.quote.as_str()) {
                reject(&mut out, "the quote is not in the stored source (a summary is not evidence)");
                continue;
            }
            if open >= MAX_PER_PAGE {
                reject(&mut out, "this page already has the most candidates it may have open (7)");
                continue;
            }
            let spec = AnchorSpec { source_id: c.source_id.clone(), locator: Some(if c.locator.trim().is_empty() { "quote".into() } else { c.locator.clone() }), quote: c.quote.clone(), ..Default::default() };
            let opts = ClaimOpts { kind: Some("attributed".into()), origin: Some(origin_of(page_id)), ..Default::default() };
            let id = match self.add_claim(&c.claim, &opts, &[]) {
                Ok(v) => v["id"].as_str().unwrap_or("").to_string(),
                Err(e) => {
                    reject(&mut out, &e);
                    continue;
                }
            };
            let had = self.claim(&id).is_some_and(|v| v["anchors"].as_array().is_some_and(|a| !a.is_empty()));
            if had {
                out.existing.push(id);
                continue;
            }
            match self.add_anchor(&id, &spec) {
                Ok(_) => {
                    open += 1;
                    out.accepted.push(id);
                }
                Err(e) => reject(&mut out, &e),
            }
        }
        out
    }

    /// The operator confirms the claim means what the source says and is true. Needs a live anchor, so the quote is still
    /// there; changes nothing else.
    pub fn attest_claim(&self, id: &str) -> Result<Value, String> {
        let mut c = self.claim(id).ok_or_else(|| format!("no claim `{}`", id))?;
        if c["origin"].as_str().is_none_or(|o| !o.starts_with("page:")) {
            return Err("only a claim taken from a summary page is attested; use `claims validate` for others".into());
        }
        if !c["anchors"].as_array().is_some_and(|a| a.iter().any(|x| x["state"].as_str().unwrap_or("active") == "active")) {
            return Err("an attested claim needs a quote that is still in a held source".into());
        }
        if !c["lifecycle"].is_null() || !c["contested_by"].as_array().is_none_or(|m| m.is_empty()) {
            return Err("the claim is contested, retired or disproved".into());
        }
        let now = delphi_core::time::now_rfc3339();
        c["attestation"] = json!({"by": "operator", "at": now, "basis": "internal source"});
        c["last_confirmed"] = json!(now);
        self.save_claim(&c)?;
        self.log("claim_attested", json!({"id": id}));
        Ok(c)
    }

    /// The operator says the claim is wrong. It leaves the queue and reads `disproved`; nothing is deleted.
    pub fn reject_claim(&self, id: &str) -> Result<Value, String> {
        let mut c = self.claim(id).ok_or_else(|| format!("no claim `{}`", id))?;
        c["lifecycle"] = json!("disproved");
        c["disproved_by"] = json!({"by": "operator", "at": delphi_core::time::now_rfc3339()});
        self.save_claim(&c)?;
        self.log("claim_rejected", json!({"id": id}));
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::claims::status;
    use delphi_core::knowledge_status::Status;

    const SRC: &str = "# Runbook\n\nThe billing service retries three times.\nPages go to the on-call.\n";

    fn setup() -> (tempfile::TempDir, Library, String) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::in_home(d.path());
        let f = d.path().join("r.md");
        std::fs::write(&f, SRC).unwrap();
        let s = l.add_file(&f, &AddOpts { licence: Some("CC0-1.0".into()), ..Default::default() }, &Budgets::default()).unwrap().id;
        (d, l, s)
    }

    fn cand(s: &str, claim: &str, quote: &str) -> Candidate {
        Candidate { claim: claim.into(), source_id: s.into(), quote: quote.into(), locator: "§1".into() }
    }

    #[test]
    fn a_candidate_needs_a_verbatim_quote_from_a_held_source_and_stays_not_validated() {
        let (_d, l, s) = setup();
        let r = l.extract_candidates("runbook", &[cand(&s, "Billing retries three times.", "retries three times"), cand(&s, "Billing retries nine times.", "retries nine times"), cand("nope", "x", "y"), cand(&s, "Empty.", " ")]);
        assert_eq!(r.accepted.len(), 1);
        assert_eq!(r.rejected.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(r.rejected[0].1.contains("not in the stored source"));
        let c = l.claim(&r.accepted[0]).unwrap();
        assert_eq!((c["kind"].as_str(), c["origin"].as_str()), (Some("attributed"), Some("page:runbook")));
        assert_eq!(status(&c), Status::Noted);
        // The same candidate again is recognised, not duplicated.
        let again = l.extract_candidates("runbook", &[cand(&s, "Billing retries three times.", "retries three times")]);
        assert_eq!((again.accepted.len(), again.existing.len()), (0, 1));
    }

    #[test]
    fn no_more_than_seven_candidates_are_open_per_page() {
        let (_d, l, s) = setup();
        let many: Vec<Candidate> = (0..9).map(|i| cand(&s, &format!("Claim number {} about retries.", i), "retries three times")).collect();
        let r = l.extract_candidates("runbook", &many);
        assert_eq!((r.accepted.len(), r.rejected.len()), (MAX_PER_PAGE, 2));
    }

    #[test]
    fn only_the_operator_attestation_validates_and_a_rejection_keeps_the_record() {
        let (_d, l, s) = setup();
        let r = l.extract_candidates("runbook", &[cand(&s, "Billing retries three times.", "retries three times"), cand(&s, "Pages go to on-call.", "Pages go to the on-call")]);
        let (a, b) = (&r.accepted[0], &r.accepted[1]);
        assert_eq!(l.open_candidates("runbook").len(), 2);
        let done = l.attest_claim(a).unwrap();
        assert_eq!(status(&done), Status::Validated);
        l.reject_claim(b).unwrap();
        assert_eq!(status(&l.claim(b).unwrap()), Status::Disproved);
        assert!(l.open_candidates("runbook").is_empty());
        assert!(l.attest_claim(b).is_err(), "a rejected claim cannot be attested");
        let plain = l.add_claim("Free text.", &ClaimOpts::default(), &[]).unwrap();
        assert!(l.attest_claim(plain["id"].as_str().unwrap()).is_err(), "not from a page");
    }
}
