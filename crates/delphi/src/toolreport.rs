//! `delphi tools`: how the team's tools are used, and how reliably the models report finished work through
//! `record_change` (`specs/033`). Reads the mission event files only; names and counts, never what was said or done.

use delphi_engine::toolreport::{render, report};
use std::path::Path;

pub struct Options {
    pub mission: Option<String>,
    pub all: bool,
    pub json: bool,
    /// The counters in the Prometheus text format instead of the table.
    pub prom: bool,
}

/// The text to print for a workspace, or what is wrong with the request.
pub fn render_for(ws_dir: &Path, opts: &Options) -> Result<String, String> {
    let ids: Vec<String> = if opts.all {
        crate::missions::list(ws_dir).into_iter().map(|r| r.id).collect()
    } else {
        match &opts.mission {
            Some(m) => vec![m.clone()],
            None => vec![crate::missions::list(ws_dir).into_iter().next().map(|r| r.id).ok_or("no missions yet: nothing has been logged")?],
        }
    };
    let mut events = Vec::new();
    for id in &ids {
        // A mission id is a directory name; refuse anything that could point elsewhere.
        if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
            return Err(format!("`{}` is not a mission id (see `delphi missions`)", id));
        }
        let dir = ws_dir.join("local/missions").join(id);
        if !dir.is_dir() {
            return Err(format!("no mission `{}` in this workspace (see `delphi missions`)", id));
        }
        events.extend(delphi_core::log::replay(&dir).map_err(|e| format!("could not read the log of {}: {}", id, e))?);
    }
    let r = report(&events);
    if opts.prom {
        return Ok(delphi_engine::toolreport::prometheus(&delphi_engine::observe::tool_stats(&events), &r));
    }
    if opts.json {
        return serde_json::to_string_pretty(&r).map(|s| s + "\n").map_err(|e| e.to_string());
    }
    let scope = if opts.all { format!("all {} mission(s)", ids.len()) } else { format!("mission {}", ids[0]) };
    Ok(render(&r, &scope))
}

pub fn cli(ws_dir: &Path, opts: &Options) -> i32 {
    match render_for(ws_dir, opts) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("delphi tools: {}", e);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_core::event::EventEnvelope;
    use serde_json::json;

    fn write_mission(ws: &Path, id: &str, span: &str, model: &str, tools: &[&str]) {
        let dir = ws.join("local/missions").join(id);
        let mut log = delphi_core::log::EventLog::open(&dir, delphi_core::log::LogOptions::default()).unwrap();
        for t in tools {
            let mut e = EventEnvelope::new(id, "tool.result", json!({"name": t, "is_error": false}));
            e.span_id = Some(span.into());
            e.model_id = Some(model.into());
            log.append(e).unwrap();
        }
    }

    #[test]
    fn reports_the_latest_mission_or_all_of_them_and_refuses_bad_ids() {
        let t = tempfile::tempdir().unwrap();
        assert!(render_for(t.path(), &Options { mission: None, all: false, json: false, prom: false }).unwrap_err().contains("no missions yet"));
        write_mission(t.path(), "m1", "s1", "model-x", &["write_file", "record_change"]);
        write_mission(t.path(), "m2", "s2", "model-y", &["write_file"]);
        let all = render_for(t.path(), &Options { mission: None, all: true, json: false, prom: false }).unwrap();
        assert!(all.contains("all 2 mission(s)") && all.contains("model-x") && all.contains("model-y") && all.contains("50%"), "{}", all);
        let one = render_for(t.path(), &Options { mission: Some("m1".into()), all: false, json: true, prom: false }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&one).unwrap();
        assert_eq!((v["total"]["worked"].as_u64(), v["total"]["recorded"].as_u64()), (Some(1), Some(1)));
        let prom = render_for(t.path(), &Options { mission: Some("m1".into()), all: false, json: false, prom: true }).unwrap();
        assert!(prom.contains("delphi_tool_calls_total{role=\"unknown\",tool=\"write_file\"} 1"), "{}", prom);
        for bad in ["../x", "a/b", ".h", ""] {
            assert!(render_for(t.path(), &Options { mission: Some(bad.into()), all: false, json: false, prom: false }).is_err(), "{}", bad);
        }
    }
}
