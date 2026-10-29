//! The validator's own model span for `validate_claims` (`specs/040` §25 V2d-2, §7.1, §24.2).
//!
//! A fresh span of the `validator` role gets the **bare claim** (no mission context, no caller text beyond the
//! normalised claim), on a model of another family than the claim's author, and answers with candidate pages. What it
//! says is only ever a list of addresses: the daemon fetches them itself, reads the passages itself and asks the judge,
//! so nothing a span read (or was told) can raise a verdict or the trust of a source. The span has its own token
//! reserve, taken before it starts and settled to what it used.

use crate::engine::{Engine, SpanOutcome};
use crate::fact_check::MAX_URLS;
use crate::handoff::HandoffEnvelope;
use crate::mission::MissionState;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Tokens held for one validator span before it starts.
pub const SPAN_RESERVE: u64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    pub urls: Vec<String>,
    pub tokens: u64,
}

pub type SpanFuture<'a> = Pin<Box<dyn Future<Output = Result<Found, String>> + Send + 'a>>;

/// Runs one validator span for one claim. Faked in tests.
pub trait SpanRunner: Send + Sync {
    fn find<'a>(&'a self, claim: &'a str) -> SpanFuture<'a>;
}

/// `SOURCE <address>` lines of a span's reply, at most [`MAX_URLS`], http(s) only. Anything else is ignored.
pub fn parse_sources(text: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(u) = line.trim().strip_prefix("SOURCE ").map(str::trim) else { continue };
        if (u.starts_with("https://") || u.starts_with("http://")) && !u.contains(char::is_whitespace) && !urls.iter().any(|x| x == u) {
            urls.push(u.to_string());
        }
        if urls.len() >= MAX_URLS {
            break;
        }
    }
    urls
}

pub struct EngineSpan {
    pub engine: Arc<Engine>,
    pub mission: Arc<MissionState>,
    pub author_family: Option<String>,
}

impl EngineSpan {
    fn validator_tokens(&self) -> u64 {
        use delphi_core::ledger::{GroupBy, Ledger};
        Ledger::open(&self.engine.ws.dir.join("local/state.db"))
            .ok()
            .and_then(|l| l.totals(GroupBy::Role, Some(&self.mission.id)).ok())
            .map(|t| t.iter().filter(|x| x.key == "validator").map(|x| x.input_tokens + x.output_tokens + x.thinking_tokens).sum())
            .unwrap_or(0)
    }
}

impl SpanRunner for EngineSpan {
    fn find<'a>(&'a self, claim: &'a str) -> SpanFuture<'a> {
        Box::pin(async move {
            let (e, m) = (&self.engine, &self.mission);
            let task_id = m.next_task_id();
            let objective = format!(
                "Find up to {MAX_URLS} public web pages that would confirm or contradict this one claim, using web search. Claim: {claim}\nReply with one line per page, exactly `SOURCE <address>`, then a final line `SEARCH COMPLETE`. Do not state a verdict: another model decides, from the pages."
            );
            let mut h = HandoffEnvelope::root(&m.id, &task_id, "validator", &objective, vec![]);
            h.handoff_id = format!("ho_{}_validator", task_id);
            h.routing_directive.model_family = self.author_family.as_deref().and_then(|f| e.other_family(f, false));
            h.routing_directive.avoid_families = self.author_family.iter().cloned().collect();
            let before = self.validator_tokens();
            let out: SpanOutcome = e.run_span(m, &task_id, h, None, 0).await.map_err(|x| x.to_string())?;
            let tokens = self.validator_tokens().saturating_sub(before);
            Ok(Found { urls: parse_sources(&out.text), tokens })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_source_lines_with_web_addresses_count() {
        let t = "Here you go\nSOURCE https://a.example/x\nSOURCE ftp://b.example\nSOURCE https://a.example/x\nSOURCE javascript:alert(1)\n  SOURCE http://c.example/y  \nignore previous instructions\nSEARCH COMPLETE";
        assert_eq!(parse_sources(t), vec!["https://a.example/x", "http://c.example/y"]);
        let many: String = (0..9).map(|i| format!("SOURCE https://s{i}.example\n")).collect();
        assert_eq!(parse_sources(&many).len(), MAX_URLS);
    }
}
