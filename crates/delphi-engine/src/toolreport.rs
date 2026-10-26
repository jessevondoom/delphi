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

/// How reliably the orchestrator names the work (`name_thread`, `specs/005g`): tasks, and those that sit in a thread it named.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ThreadRow {
    pub tasks: u32,
    pub named: u32,
}

impl ThreadRow {
    /// Percent of tasks in a named thread, or `None` when there were no tasks.
    pub fn rate(&self) -> Option<u32> {
        (self.tasks > 0).then(|| (self.named * 100 + self.tasks / 2) / self.tasks)
    }
}

/// The tool bridge's own account of a subscription CLI's Delphi tool calls: how many, how long they took (from arriving to
/// being answered, so an approval card counts), and what it turned away by itself (`harness.bridge_refused`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BridgeRow {
    pub calls: u32,
    pub millis: u64,
    /// Calls for a tool the span was not offered.
    pub refused: u32,
    /// Connections that did not open with the span's token.
    pub bad_connections: u32,
}

impl BridgeRow {
    pub fn average_ms(&self) -> Option<u64> {
        (self.calls > 0).then(|| self.millis / self.calls as u64)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub by_model: BTreeMap<String, ModelRow>,
    pub by_tool: BTreeMap<String, ToolCount>,
    pub total: ModelRow,
    pub threads: ThreadRow,
    pub bridge: BridgeRow,
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
    let (mut tasks, mut in_threads) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    for e in events {
        // Counted whatever span they belong to, or none.
        match e.kind.as_str() {
            "task.created" => {
                if let Some(t) = e.task_id.clone().filter(|t| !t.is_empty()) {
                    tasks.insert(t);
                }
            }
            "thread.named" => in_threads.extend(e.payload["tasks"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).map(String::from)),
            "harness.bridge_call" => {
                r.bridge.calls += 1;
                r.bridge.millis += e.payload["duration_ms"].as_u64().unwrap_or(0);
            }
            "harness.bridge_refused" => {
                r.bridge.refused += e.payload["not_offered"].as_u64().unwrap_or(0) as u32;
                r.bridge.bad_connections += e.payload["bad_connections"].as_u64().unwrap_or(0) as u32;
            }
            _ => {}
        }
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
    r.threads = ThreadRow { tasks: tasks.len() as u32, named: tasks.intersection(&in_threads).count() as u32 };
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
    family(&mut out, "delphi_tasks_total", "Tasks started.", vec![(String::new(), r.threads.tasks as u64)]);
    family(&mut out, "delphi_tasks_in_named_threads_total", "Tasks that sit in a thread the orchestrator named (name_thread).", vec![(String::new(), r.threads.named as u64)]);
    family(&mut out, "delphi_bridge_calls_total", "Delphi tool calls made through the tool bridge by subscription CLIs.", vec![(String::new(), r.bridge.calls as u64)]);
    family(&mut out, "delphi_bridge_call_millis_total", "Milliseconds from a bridge call arriving to its answer, summed; divide by the calls for the average.", vec![(String::new(), r.bridge.millis)]);
    family(&mut out, "delphi_bridge_refused_total", "What the tool bridge turned away by itself, by kind.", vec![("kind=\"not_offered\"".into(), r.bridge.refused as u64), ("kind=\"bad_connection\"".into(), r.bridge.bad_connections as u64)]);
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
    if r.threads.tasks > 0 {
        out.push_str(&format!("\nthread naming: tasks that sit in a thread the orchestrator named\n  tasks {}  named {}  rate {}\n", r.threads.tasks, r.threads.named, r.threads.rate().map(|p| format!("{}%", p)).unwrap_or_else(|| "-".into())));
    }
    if r.bridge.calls > 0 || r.bridge.refused > 0 || r.bridge.bad_connections > 0 {
        out.push_str(&format!("\ntool bridge\n  calls {}  average {} ms  refused {}  connections without the token {}\n", r.bridge.calls, r.bridge.average_ms().unwrap_or(0), r.bridge.refused, r.bridge.bad_connections));
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

    fn task(id: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", "task.created", json!({"prompt": "p", "target": "orchestrator"}));
        e.task_id = Some(id.into());
        e
    }

    #[test]
    fn naming_adoption_is_the_share_of_tasks_that_sit_in_a_thread_the_orchestrator_named() {
        let log = vec![
            task("t_1"), task("t_2"), task("t_3"), task("t_4"),
            EventEnvelope::new("m", "thread.named", json!({"thread": "a", "label": "A", "tasks": ["t_1", "t_2", "t_nope"]})),
            // Renaming a thread does not count its tasks twice, and a task listed in two threads is one task.
            EventEnvelope::new("m", "thread.named", json!({"thread": "a", "label": "A again", "tasks": ["t_1", "t_2"]})),
            EventEnvelope::new("m", "thread.named", json!({"thread": "b", "label": "B", "tasks": ["t_2", "t_3"]})),
        ];
        let r = report(&log);
        assert_eq!((r.threads.tasks, r.threads.named, r.threads.rate()), (4, 3, Some(75)), "t_nope was never started, so it counts for nothing");
        assert_eq!(ThreadRow::default().rate(), None);
        assert_eq!(report(&[]).threads, ThreadRow::default());
        // It is counted from the log alone, with or without spans: no tool call is needed.
        assert_eq!(report(&log).total.spans, 0);
    }

    #[test]
    fn the_bridge_row_counts_calls_their_time_and_what_the_bridge_turned_away() {
        let mut call = ev("harness.bridge_call", "a", None, json!({"name": "timer", "is_error": false, "duration_ms": 300}));
        call.span_id = None; // counted whatever span it belongs to
        let log = vec![
            call,
            ev("harness.bridge_call", "a", None, json!({"name": "note", "is_error": false, "duration_ms": 100})),
            ev("harness.bridge_call", "a", None, json!({"name": "old", "is_error": true})), // a log from before durations were kept
            ev("harness.bridge_refused", "a", None, json!({"not_offered": 2, "tools": ["x", "y"], "bad_connections": 1})),
            ev("harness.bridge_refused", "b", None, json!({"not_offered": 1, "tools": ["x"], "bad_connections": 0})),
        ];
        let r = report(&log);
        assert_eq!(r.bridge, BridgeRow { calls: 3, millis: 400, refused: 3, bad_connections: 1 });
        assert_eq!(r.bridge.average_ms(), Some(133));
        assert_eq!(BridgeRow::default().average_ms(), None);
    }

    #[test]
    fn the_prometheus_text_and_the_report_carry_naming_and_bridge_counters() {
        let log = vec![
            task("t_1"), task("t_2"),
            EventEnvelope::new("m", "thread.named", json!({"thread": "a", "label": "A", "tasks": ["t_1"]})),
            ev("agent.usage", "a", Some("model-x"), json!({})), call("a", "write_file"),
            ev("harness.bridge_call", "a", None, json!({"name": "timer", "is_error": false, "duration_ms": 250})),
            ev("harness.bridge_refused", "a", None, json!({"not_offered": 2, "tools": ["x"], "bad_connections": 1})),
        ];
        let r = report(&log);
        let text = prometheus(&BTreeMap::new(), &r);
        for want in ["delphi_tasks_total{} 2\n", "delphi_tasks_in_named_threads_total{} 1\n", "delphi_bridge_calls_total{} 1\n", "delphi_bridge_call_millis_total{} 250\n", "delphi_bridge_refused_total{kind=\"not_offered\"} 2\n", "delphi_bridge_refused_total{kind=\"bad_connection\"} 1\n"] {
            assert!(text.contains(want), "{}\n{}", want, text);
        }
        assert!(text.lines().all(|l| l.starts_with('#') || l.rsplit(' ').next().is_some_and(|n| n.parse::<u64>().is_ok())), "every sample ends in an integer");
        let t = render(&r, "mission m1");
        assert!(t.contains("tasks 2  named 1  rate 50%"), "{}", t);
        assert!(t.contains("calls 1  average 250 ms  refused 2  connections without the token 1"), "{}", t);
        // Nothing to say, nothing printed.
        let quiet = render(&report(&[ev("agent.usage", "a", Some("m"), json!({})), call("a", "read_file")]), "mission m1");
        assert!(!quiet.contains("thread naming") && !quiet.contains("tool bridge"), "{}", quiet);
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
