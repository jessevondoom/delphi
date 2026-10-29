//! The recheck on open (`specs/055` §3.5; closes an SK1 gap).
//!
//! A source opened more than 30 days after `last_checked` is fetched once through the V2a fetcher (SSRF guard, size cap,
//! cloaking check, no tokens). The fetcher takes no request headers, so the change test is the spec's fallback: a hash
//! comparison of the fresh fetch. Unchanged moves `last_checked` and nothing else; changed is a new version with the old
//! one kept under `versions/` and a change note; gone or moved keeps the last capture and marks the source retired.
//! Nothing is overwritten.

use super::*;

pub const RECHECK_AFTER_DAYS: i64 = 30;
/// Rechecks in one calendar day. The per-host politeness delay is moot while a recheck is one fetch for one open.
pub const DAILY_CAP: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recheck {
    /// Checked within the last 30 days.
    Fresh,
    /// Nothing to fetch: a file, a bundle's source, a purged capture.
    NotApplicable(&'static str),
    /// The network was unreachable; nothing changed, and the open says so.
    Offline,
    CapReached,
    Unchanged,
    /// A new version was stored; the earlier one is under `versions/`.
    Changed(u64),
    /// Gone (404 or 410): the last capture is kept and the source is marked retired.
    Retired,
}

impl Library {
    pub async fn recheck_on_open(&self, policy: &FetchPolicy, id: &str, budgets: &Budgets) -> Result<Recheck, String> {
        let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap_or(0);
        self.recheck_at(policy, id, budgets, now).await
    }

    pub async fn recheck_at(&self, policy: &FetchPolicy, id: &str, budgets: &Budgets, now: i64) -> Result<Recheck, String> {
        let meta = self.read_meta(id).ok_or_else(|| format!("no source `{}`", id))?;
        if !meta["purged"].is_null() {
            return Ok(Recheck::NotApplicable("the capture was purged"));
        }
        let Some(url) = meta["canonical_url"].as_str().filter(|u| u.starts_with("http")).map(String::from) else {
            return Ok(Recheck::NotApplicable("no URL to check"));
        };
        let last = meta["last_checked"].as_str().or(meta["retrieved"].as_str()).and_then(delphi_core::time::parse_rfc3339);
        if last.is_some_and(|l| now - l <= RECHECK_AFTER_DAYS * 86_400) {
            return Ok(Recheck::Fresh);
        }
        let today = delphi_core::time::now_rfc3339()[..10].to_string();
        let done_today = std::fs::read_to_string(self.root.join("log.jsonl")).unwrap_or_default().lines().filter(|l| l.contains("\"event\":\"recheck\"") && l.contains(&format!("\"ts\":\"{}", today))).count();
        if done_today >= DAILY_CAP {
            return Ok(Recheck::CapReached);
        }
        self.log("recheck", json!({"id": id, "url": url}));
        let opts = AddOpts {
            licence: meta["licence"].as_str().map(String::from),
            level: meta["capture_level"].as_str().and_then(Level::parse),
            private: is_private(&meta),
            rights_holder: meta["rights_holder"].as_str().map(String::from),
            attribution_text: meta["attribution_text"].as_str().map(String::from),
            doi: meta["DOI"].as_str().map(String::from),
            ..Default::default()
        };
        match self.add_url(policy, &url, &opts, budgets).await {
            Ok(s) => {
                let mut m = self.read_meta(id).unwrap_or(meta);
                if !m["retired"].is_null() {
                    m.as_object_mut().map(|o| o.remove("retired"));
                    self.write_json(&self.source_dir(id).join("meta.json"), &m)?;
                    self.source_revived(id);
                }
                Ok(match s.outcome {
                    Outcome::NewVersion(n) => Recheck::Changed(n),
                    _ => Recheck::Unchanged,
                })
            }
            // `Refusal`'s display is all the fetcher gives back
            Err(e) if e == "http status 404" || e == "http status 410" => {
                let mut m = meta;
                m["retired"] = json!(delphi_core::time::now_rfc3339());
                self.write_json(&self.source_dir(id).join("meta.json"), &m)?;
                self.source_retired(id);
                self.log("retired", json!({"id": id}));
                Ok(Recheck::Retired)
            }
            Err(e) if e == "network error or timeout" => Ok(Recheck::Offline),
            Err(e) => Err(e),
        }
    }
}
