//! Tool use measured from the mission logs (`specs/033`): how often each tool is called and fails, and how reliably the
//! models report finished work through `record_change` (`specs/032`). Pure: events in, a report out. It reads names,
//! counts and event kinds only; no arguments, results or text.

use crate::observe::{tool_call_of, ToolCount};
use delphi_core::event::EventEnvelope;
use serde::Serialize;
use std::collections::BTreeMap;

/// Tools that change something, by the names Delphi's own tools and the CLIs give them. A span that called one did work.
const WORK_TOOLS: [&str; 15] = [
    "write_file", "edit_file", "run_command", "write_archive", "write_html", "write_chart", "write_report", "write_deck", "write_sheet", "write_diagram", "Edit", "Write", "MultiEdit", "NotebookEdit", "Bash",
];

/// What `record_change` adoption is measured against.
pub const RECORD_TOOL: &str = "record_change";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModelRow {
    /// Agent runs (spans) that made at least one tool call.
    pub spans: u32,
    pub calls: u32,
    pub errors: u32,
    /// Runs that changed something (a write, an edit, a command).
    pub worked: u32,
    /// Of those, runs that recorded at least one change.
    pub recorded: u32,
}

impl ModelRow {
    /// Percent of working runs that recorded a change, or `None` when none worked.
    pub fn rate(&self) -> Option<u32> {
        (self.worked > 0).then(|| (self.recorded * 100 + self.worked / 2) / self.worked)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub by_model: BTreeMap<String, ModelRow>,
    pub by_tool: BTreeMap<String, ToolCount>,
    pub total: ModelRow,
}

#[derive(Default)]
struct Span {
    model: String,
    calls: u32,
    errors: u32,
    worked: bool,
    recorded: bool,
}

/// The report for the events of one or more missions. A span's model comes from its `agent.usage` (or any event that
/// names one); a span with none is filed under `unknown`.
pub fn report(events: &[EventEnvelope]) -> Report {
    let mut spans: BTreeMap<String, Span> = BTreeMap::new();
    let mut r = Report::default();
    for e in events {
        let Some(id) = e.span_id.clone().filter(|s| !s.is_empty()) else { continue };
        if let Some(m) = e.model_id.as_deref().filter(|m| !m.is_empty()) {
            spans.entry(id.clone()).or_default().model = m.to_string();
        }
        if e.kind == "change.recorded" {
            spans.entry(id.clone()).or_default().recorded = true;
        }
        let Some((tool, failed)) = tool_call_of(&e.kind, &e.payload) else { continue };
        let s = spans.entry(id).or_default();
        s.calls += 1;
        s.errors += failed as u32;
        s.worked |= WORK_TOOLS.contains(&tool.as_str());
        s.recorded |= tool == RECORD_TOOL;
        let c = r.by_tool.entry(tool).or_default();
        c.calls += 1;
        c.errors += failed as u32;
    }
    for s in spans.into_values().filter(|s| s.calls > 0) {
        let key = if s.model.is_empty() { "unknown".to_string() } else { s.model.clone() };
        for row in [r.by_model.entry(key).or_default(), &mut r.total] {
            row.spans += 1;
            row.calls += s.calls;
            row.errors += s.errors;
            row.worked += s.worked as u32;
            row.recorded += (s.worked && s.recorded) as u32;
        }
    }
    r
}

/// A label value in the Prometheus text format: backslash, quote and newline escaped.
fn label(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// The counters in the Prometheus text exposition format, for a scrape or a node exporter's textfile collector: tool calls
/// and failures by role and tool (a role's ten most used tools, and always `record_change`), recorded changes by role, and
/// runs by model (all, that changed something, and of those that recorded a change). Counts only.
pub fn prometheus(stats: &BTreeMap<String, crate::observe::RoleTools>, r: &Report) -> String {
    let mut out = String::new();
    let family = |out: &mut String, name: &str, help: &str, rows: Vec<(String, u64)>| {
        out.push_str(&format!("# HELP {} {}\n# TYPE {} counter\n", name, help, name));
        for (labels, n) in rows {
            out.push_str(&format!("{}{{{}}} {}\n", name, labels, n));
        }
    };
    let by_tool = |f: fn(&ToolCount) -> u32| -> Vec<(String, u64)> {
        stats.iter().flat_map(|(role, t)| t.by_tool.iter().map(move |(tool, c)| (format!("role=\"{}\",tool=\"{}\"", label(role), label(tool)), f(c) as u64))).collect()
    };
    family(&mut out, "delphi_tool_calls_total", "Tool calls by role and tool.", by_tool(|c| c.calls));
    family(&mut out, "delphi_tool_errors_total", "Failed tool calls by role and tool.", by_tool(|c| c.errors));
    family(&mut out, "delphi_changes_recorded_total", "Changes agents recorded with record_change, by role.", stats.iter().filter_map(|(role, t)| t.by_tool.get(RECORD_TOOL).map(|c| (format!("role=\"{}\"", label(role)), c.calls as u64))).collect());
    let by_model = |f: fn(&ModelRow) -> u32| -> Vec<(String, u64)> { r.by_model.iter().map(|(m, row)| (format!("model=\"{}\"", label(m)), f(row) as u64)).collect() };
    family(&mut out, "delphi_runs_total", "Agent runs that made at least one tool call, by model.", by_model(|m| m.spans));
    family(&mut out, "delphi_runs_worked_total", "Runs that changed something (a write, an edit, a command), by model.", by_model(|m| m.worked));
    family(&mut out, "delphi_runs_recorded_total", "Runs that changed something and recorded a change, by model.", by_model(|m| m.recorded));
    out
}

/// The report as text for a terminal.
pub fn render(r: &Report, scope: &str) -> String {
    if r.total.spans == 0 {
        return format!("No tool calls were logged in {}.\n", scope);
    }
    let pct = |row: &ModelRow| row.rate().map(|p| format!("{}%", p)).unwrap_or_else(|| "-".into());
    let w = r.by_model.keys().map(|k| k.chars().count()).max().unwrap_or(5).max(5);
    let mut out = format!("Tool use in {}\n\nrecord_change adoption: runs that changed something and recorded a change\n", scope);
    out.push_str(&format!("  {:<w$}  {:>5}  {:>6}  {:>8}  {:>5}  {:>6}  {:>6}\n", "model", "runs", "worked", "recorded", "rate", "calls", "failed", w = w));
    for (k, m) in r.by_model.iter().chain(std::iter::once((&"all".to_string(), &r.total))) {
        out.push_str(&format!("  {:<w$}  {:>5}  {:>6}  {:>8}  {:>5}  {:>6}  {:>6}\n", k, m.spans, m.worked, m.recorded, pct(m), m.calls, m.errors, w = w));
    }
    out.push_str("\ntools\n");
    let mut tools: Vec<_> = r.by_tool.iter().collect();
    tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then_with(|| a.0.cmp(b.0)));
    for (name, c) in tools.into_iter().take(25) {
        out.push_str(&format!("  {:<28}{:>6}{:>8}\n", name, c.calls, if c.errors > 0 { format!("{} failed", c.errors) } else { String::new() }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str, span: &str, model: Option<&str>, payload: serde_json::Value) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", kind, payload);
        e.span_id = Some(span.into());
        e.agent = Some("builder".into());
        e.model_id = model.map(String::from);
        e
    }
    fn call(span: &str, tool: &str) -> EventEnvelope {
        ev("tool.result", span, None, json!({"name": tool, "is_error": false}))
    }

    #[test]
    fn adoption_is_the_share_of_runs_that_changed_something_and_recorded_it_by_model() {
        let log = vec![
            ev("agent.usage", "a", Some("model-x"), json!({})),
            call("a", "read_file"),
            call("a", "write_file"),
            call("a", "record_change"),
            ev("agent.usage", "b", Some("model-x"), json!({})),
            call("b", "run_command"),
            ev("agent.usage", "c", Some("model-y"), json!({})),
            call("c", "read_file"), // read-only: did no work, so it is not expected to record anything
            ev("harness.tool", "d", Some("model-y"), json!({"tool": "Edit", "summary": ""})),
            ev("harness.bridge_call", "d", Some("model-y"), json!({"name": "mcp__delphi__record_change", "is_error": false})),
            call("e", "write_file"), // no model logged for this span
            ev("change.recorded", "f", Some("model-y"), json!({"summary": "x"})), // recorded but called nothing: not a run with calls
        ];
        let r = report(&log);
        assert_eq!(r.by_model["model-x"], ModelRow { spans: 2, calls: 4, errors: 0, worked: 2, recorded: 1 });
        assert_eq!(r.by_model["model-y"], ModelRow { spans: 2, calls: 3, errors: 0, worked: 1, recorded: 1 });
        assert_eq!(r.by_model["unknown"].worked, 1);
        assert_eq!((r.total.spans, r.total.worked, r.total.recorded, r.total.rate()), (5, 4, 2, Some(50)));
        assert_eq!(r.by_model["model-x"].rate(), Some(50));
        assert_eq!(r.by_model["model-y"].rate(), Some(100));
        assert_eq!(r.by_tool["record_change"].calls, 2);
        assert_eq!(ModelRow::default().rate(), None);
    }

    #[test]
    fn the_prometheus_text_has_counters_by_role_tool_and_model_and_escapes_labels() {
        let mut stats = BTreeMap::new();
        let mut t = crate::observe::RoleTools::default();
        t.by_tool.insert("read_file".into(), ToolCount { calls: 3, errors: 1 });
        t.by_tool.insert("record_change".into(), ToolCount { calls: 2, errors: 0 });
        stats.insert("builder".to_string(), t);
        let log = vec![ev("agent.usage", "a", Some("model \"x\""), json!({})), call("a", "write_file"), call("a", "record_change")];
        let text = prometheus(&stats, &report(&log));
        assert!(text.contains("# TYPE delphi_tool_calls_total counter\ndelphi_tool_calls_total{role=\"builder\",tool=\"read_file\"} 3\n"), "{}", text);
        assert!(text.contains("delphi_tool_errors_total{role=\"builder\",tool=\"read_file\"} 1\n"));
        assert!(text.contains("delphi_changes_recorded_total{role=\"builder\"} 2\n"));
        assert!(text.contains("delphi_runs_recorded_total{model=\"model \\\"x\\\"\"} 1\n"), "{}", text);
        assert!(text.lines().all(|l| l.starts_with('#') || l.rsplit(' ').next().is_some_and(|n| n.parse::<u64>().is_ok())), "every sample ends in an integer");
    }

    #[test]
    fn the_text_has_one_row_per_model_a_total_and_the_busiest_tools() {
        let log = vec![ev("agent.usage", "a", Some("model-x"), json!({})), call("a", "write_file"), call("a", "read_file"), call("a", "read_file")];
        let t = render(&report(&log), "mission m1");
        assert!(t.contains("model-x") && t.contains("  0%") && t.contains("all"), "{}", t);
        assert!(t.find("read_file").unwrap() < t.find("write_file").unwrap(), "busiest first: {}", t);
        assert_eq!(render(&Report::default(), "mission m1"), "No tool calls were logged in mission m1.\n");
    }
}
