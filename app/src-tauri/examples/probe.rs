//! `cargo run --example probe`: find the daemon as the app does, attach read-only, and print what it answers. No window.
//! It sends only the reads the app is allowed (spec 111 §2.2) and prints counts and names, never an agent's text.

use serde_json::json;

use delphi_app_lib::client::{Client, Method};
use delphi_app_lib::locate;

fn main() {
    let Some(found) = locate::locate() else {
        println!("no Delphi home was found");
        std::process::exit(1);
    };
    println!("narrator socket: {}", found.narrator.display());
    println!("delphi: {} · workspace: {} · roles: {}", found.info["version"].as_str().unwrap_or("not asked"), found.info["workspace_name"].as_str().unwrap_or("?"), found.info["roles"].as_object().map_or(0, |r| r.len()));
    let mut client = match Client::attach(&found.narrator, env!("CARGO_PKG_VERSION")) {
        Ok(c) => c,
        Err(e) => {
            println!("could not attach: {e}");
            std::process::exit(1);
        }
    };
    println!("attached on the narrator scope");
    match client.call(Method::ObserveState, json!({})) {
        Ok(s) => {
            let count = |k: &str| s[k].as_object().map(|o| o.len()).or_else(|| s[k].as_array().map(|a| a.len())).unwrap_or(0);
            println!("observe.state: mission {} · agents {} · tasks {} · councils {} · changes {} · waiting on the operator {}", s["mission_id"].as_str().unwrap_or("none"), count("agents"), count("tasks"), count("councils"), count("changes"), count("approvals"));
            let mut keys: Vec<&str> = s.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            keys.sort_unstable();
            println!("fields: {}", keys.join(" "));
        }
        Err(e) => println!("observe.state: {e}"),
    }
    match client.call(Method::ObserveSnapshot, json!({})) {
        Ok(s) => {
            let events = s["events"].as_array().cloned().unwrap_or_default();
            let mut kinds: Vec<String> = events.iter().filter_map(|e| e["type"].as_str().map(String::from)).collect();
            kinds.sort_unstable();
            kinds.dedup();
            println!("observe.snapshot: {} events of kinds: {}", events.len(), kinds.join(" "));
        }
        Err(e) => println!("observe.snapshot: {e}"),
    }
    println!("session.ping: {:?}", client.call(Method::Ping, json!({})).map(|v| v.to_string()).map_err(|e| e.to_string()));
}
