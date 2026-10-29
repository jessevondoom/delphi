//! The publication confirmation (`specs/023` §2.1, `specs/055` §11.1 item 3).
//!
//! A source's licence never refuses a bundle; it informs the person who sends the bundle outside the org. When
//! `bundle package` runs for a `public` bundle, or `bundle publish` (the Xenia store upload) runs for any bundle, and a
//! source carries a licence warning or the bundle is `internal`, the command lists the affected source ids with their
//! class and asks once. The confirmation is the `sign` precedent: a terminal on stdin and stdout, no `--yes`, no
//! environment variable, so a process an agent started is refused. The acknowledged ids and the time are written to
//! `.delphi-bundle/report.json` as `licence_acknowledged`; after that the command goes on and the content is never refused.

use delphi_bundle::{sources, Host, Manifest, Options, Report, Tree};
use serde_json::{json, Value};
use std::io::BufRead;
use std::path::Path;

/// What the operator is shown, and the ids they acknowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// `(source id, licence class)`.
    pub items: Vec<(String, String)>,
    pub internal: bool,
    pub text: String,
}

fn class_of(licence: &str) -> String {
    match sources::classify(licence) {
        sources::Licence::Open { .. } => "open".into(),
        sources::Licence::Internal => "internal".into(),
        sources::Licence::Restricted(_) => "restricted".into(),
    }
}

/// The sources the confirmation covers, `None` when there is nothing to acknowledge: a `source-*` warning names its
/// source; an `internal` bundle names all of them. A bundle without sources has nothing to acknowledge.
pub fn notice(dir: &Path, r: &Report, m: &Manifest) -> Option<Notice> {
    let internal = m.distribution == "internal";
    let tree = Tree::open(dir);
    let listed = sources::list(&tree);
    let warned: Vec<&str> = r.warns.iter().filter(|w| w.msg.starts_with("source-")).filter_map(|w| w.at.split('/').nth(1)).collect();
    let items: Vec<(String, String)> = listed.iter().filter(|s| internal || warned.contains(&s.id.as_str())).map(|s| (s.id.clone(), class_of(if s.licence == "?" { "" } else { &s.licence }))).collect();
    if items.is_empty() {
        return None;
    }
    let mut text = String::from("sources that carry a licence note:\n");
    for (id, class) in &items {
        text.push_str(&format!("  {}  ({})\n", id, class));
    }
    if internal {
        text.push_str("this bundle is marked internal\n");
    }
    text.push_str("Delphi does not refuse content for its licence; sending it outside the org is your judgement.");
    Some(Notice { items, internal, text })
}

/// The real confirmation: a terminal on both ends, and `publish` typed back. There is no other way to answer.
pub fn tty_confirm(n: &Notice) -> Result<bool, String> {
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !on_terminal {
        return Err("publishing sources outside the org is a human act: run this in a terminal (it has no non-interactive mode, no --yes and no environment switch)".into());
    }
    println!("{}", n.text);
    print!("type publish to go ahead, anything else cancels: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim() == "publish")
}

/// Ask once if there is something to acknowledge, and record the answer in `.delphi-bundle/report.json`. `Ok` means go on.
pub fn gate(dir: &Path, r: &Report, m: &Manifest, confirm: &dyn Fn(&Notice) -> Result<bool, String>) -> Result<(), String> {
    let Some(n) = notice(dir, r, m) else { return Ok(()) };
    if !confirm(&n)? {
        return Err("not confirmed; nothing was sent".into());
    }
    let path = dir.join(".delphi-bundle/report.json");
    let mut report = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let mut seen: Vec<Value> = report["licence_acknowledged"].as_array().cloned().unwrap_or_default();
    for (id, class) in &n.items {
        let one = json!({"id": id, "class": class});
        if !seen.contains(&one) {
            seen.push(one);
        }
    }
    report["licence_acknowledged"] = Value::Array(seen);
    report["licence_acknowledged_at"] = json!(delphi_core::time::now_rfc3339());
    std::fs::create_dir_all(dir.join(".delphi-bundle")).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    delphi_core::lock::write_atomic(&path, &body).map_err(|e| format!("report.json: {}", e))
}

/// `gate` for a command that has only the directory (the Xenia upload): validate as `package` would, then ask. Only a `public` bundle is asked; an `internal` one stays in the org.
pub fn gate_dir(dir: &Path, host: &dyn Host, confirm: &dyn Fn(&Notice) -> Result<bool, String>) -> Result<(), String> {
    let r = delphi_bundle::validate(dir, &Options::default(), host);
    match &r.manifest {
        Some(m) if m.distribution == "public" => gate(dir, &r, m, confirm),
        Some(_) => Ok(()),
        None => Ok(()),
    }
}
