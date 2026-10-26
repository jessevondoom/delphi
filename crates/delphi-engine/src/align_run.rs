//! `check_alignment`: the alignment pre-pass (`align.rs`) as a tool, for the roles that plan and review.
//!
//! It is offered to a role whose manifest lists `org-context` (the same set that is shown the slice) and only while a bundle
//! is added. The document is read through the same guarded path as `read_file`, so scope, tier and taint behave
//! identically; what comes back is derived text (ids and findings), which advises and blocks nothing (`specs/026` §5).

use crate::engine::{tool_result_error, Engine, SpanCtx};
use crate::mission::EngineError;
use crate::tools::{self, ToolResult};
use delphi_adapter::claude::ToolDef;
use delphi_core::event::EventEnvelope;
use serde_json::{json, Value};

pub(crate) const TOOL: &str = "check_alignment";

pub(crate) fn def() -> ToolDef {
    ToolDef {
        name: TOOL.into(),
        description: "Check which goals, perspectives and decisions of your organization's bundles a document cites (for example `[G-acme-01]`, `knowledge/decisions/0042-…`), and whether each still stands: superseded, not defined by any added bundle, or past its review date or horizon. Deterministic and free. Findings are advice (P2): they never block anything. Use it on a spec or a review before you judge alignment.".into(),
        input_schema: json!({"type": "object", "required": ["path"], "properties": {"path": {"type": "string"}}}),
    }
}

impl Engine {
    /// Does this span get the tool: a role with `org-context`, tools, and at least one added bundle.
    pub(crate) fn offers_alignment(&self, sc: &SpanCtx) -> bool {
        sc.manifest.execute.iter().any(|e| e == "org-context") && !sc.toolless && !delphi_core::bundles::active_entries(&self.ws.dir).is_empty()
    }

    pub(crate) async fn exec_align(&self, sc: &mut SpanCtx, args: &Value) -> Result<ToolResult, EngineError> {
        if !sc.manifest.execute.iter().any(|e| e == "org-context") {
            return Ok(tool_result_error("this role's manifest does not authorize org-context"));
        }
        if delphi_core::bundles::active_entries(&self.ws.dir).is_empty() {
            return Ok(tool_result_error("no bundle is added, so there is nothing to check against"));
        }
        let Some(path) = args["path"].as_str().filter(|p| !p.is_empty() && p.len() <= 1024) else {
            return Ok(tool_result_error("check_alignment needs a `path`"));
        };
        let before = sc.span.ceiling();
        let now = self.now();
        let (read, stamps) = {
            let mut idx = sc.m.index.lock().unwrap();
            let mut ctx = tools::ToolCtx { guard: &sc.guard, span: &mut sc.span, index: &mut idx, rules: &self.rules, probe: &*self.probe, role: &sc.role, mission_id: &sc.m.id, now_rfc3339: &now, stamps: vec![], events: vec![] };
            let r = tools::read_pointer(&mut ctx, path);
            (r, std::mem::take(&mut ctx.stamps))
        };
        let mut events: Vec<EventEnvelope> = stamps.iter().map(|s| sc.ev("provenance.stamp", delphi_core::provenance::Index::stamp_payload(s.zone, &s.path, &s.blake3, s.tier))).collect();
        if sc.span.ceiling() != before {
            events.push(sc.ev("span.taint", json!({"ceiling": sc.span.ceiling().as_str(), "cause": TOOL})));
        }
        let result = match read {
            Err(e) => tool_result_error(e.to_string()),
            Ok((text, _tier)) => {
                let (findings, notes) = crate::align::prepass(&self.ws, &text, &now[..10]);
                let mut out = if findings.is_empty() { format!("{}: no cited goal, perspective or decision of an added bundle is superseded, unknown or past its review.\n", path) } else { format!("{}: {} finding(s), all P2 (advice, nothing is blocked):\n{}\n", path, findings.len(), findings.iter().map(|f| f.line()).collect::<Vec<_>>().join("\n")) };
                for n in notes {
                    out.push_str(&format!("note: {}\n", n));
                }
                events.push(sc.ev("align.finding", json!({"path": path, "findings": findings.iter().map(|f| json!({"code": f.code, "level": f.level, "id": f.id})).collect::<Vec<_>>()})));
                ToolResult::ok(out)
            }
        };
        sc.m.append_batch(events).await?;
        Ok(result)
    }
}
