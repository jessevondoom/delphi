//! Chart specs used by the tests and by `write_samples`, which renders them for a person (or a browser) to look at.

use serde_json::{json, Value};

pub fn all() -> Vec<(&'static str, Value)> {
    let days: Vec<Value> = (0..60).map(|d| json!([format!("2026-{:02}-{:02}", 8 + d / 31, 1 + d % 31), 1200.0 + (d as f64 * 0.35).sin() * 140.0 + d as f64 * 9.0])).collect();
    let scatter_rows: Vec<Value> = (0..60).map(|i| { let x = i as f64 * 1.7 + (i as f64 * 1.3).sin() * 6.0; json!([x, 20.0 + x * 0.8 + (i as f64 * 2.1).cos() * 9.0, if i % 3 == 0 { "arm64" } else { "x86_64" }]) }).collect();
    let hist_rows: Vec<Value> = (0..240).map(|i| { let a = (i as f64 * 0.37).sin() * 20.0 + (i as f64 * 0.11).cos() * 15.0; json!([i, 100.0 + a + (i % 17) as f64 * 3.0]) }).collect();
    vec![
        ("line-two-series", json!({
            "type": "line", "title": "Median latency fell 12% in 0.2.3", "subtitle": "ms, lower is better",
            "data": {"columns": ["release", "p50", "p95"], "rows": [["0.2.1", 120, 340], ["0.2.2", 118, 310], ["0.2.3", 104, 255], ["0.2.4", 103, 262], ["0.2.5", 101, 248]]},
            "x": "release", "y": ["p50", "p95"], "x_order": "ordered", "format": {"y": "0,0 ms"},
            "annotations": [{"x": "0.2.3", "label": "daemon rewrite"}], "source": "missions/m1/latency.csv"})),
        ("hbar-single", json!({
            "type": "hbar", "title": "Most calls go to three endpoints", "subtitle": "requests per day",
            "data": {"columns": ["endpoint", "calls"], "rows": [["/v1/messages", 48200], ["/v1/models", 21300], ["/v1/complete", 18900], ["/healthz", 5200], ["/v1/batches", 2100], ["/v1/files/upload-large", 640]]},
            "x": "endpoint", "y": "calls"})),
        ("bar-grouped-three", json!({
            "type": "bar", "title": "Builds are slowest on Windows", "subtitle": "minutes per build",
            "data": {"columns": ["quarter", "linux", "macos", "windows"], "rows": [["Q1", 4.2, 6.1, 9.8], ["Q2", 4.0, 5.8, 9.1], ["Q3", 3.7, 5.2, 8.4]]},
            "x": "quarter", "format": {"y": "0.0"}})),
        ("area-dates", json!({
            "type": "area", "title": "Daily active users grew through the summer",
            "data": {"columns": ["day", "users"], "rows": days}, "x": "day", "y": "users", "scale": {"y_zero": false}})),
        ("scatter-two", json!({
            "type": "scatter", "title": "Build time grows with changed lines", "subtitle": "seconds vs lines changed (hundreds)",
            "data": {"columns": ["lines", "seconds", "arch"], "rows": scatter_rows}, "x": "lines", "y": "seconds", "series_by": "arch"})),
        ("hist", json!({
            "type": "hist", "title": "Most requests finish within 140 ms", "subtitle": "request latency, ms",
            "data": {"columns": ["i", "ms"], "rows": hist_rows}, "y": "ms"})),
        ("line-four-converging", json!({
            "type": "line", "title": "Four services converge after the cache change",
            "data": {"columns": ["week", "api", "web", "worker", "batch"], "rows": [[1, 90, 70, 50, 30], [2, 88, 72, 55, 36], [3, 85, 74, 62, 45], [4, 80, 75, 68, 60], [5, 77, 76, 73, 72], [6, 76, 76, 75, 75]]},
            "x": "week", "format": {"y": "0\"%\""}, "source": "dashboards/weekly"})),
    ]
}
