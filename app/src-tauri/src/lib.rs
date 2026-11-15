//! Delphi desktop (spec 111): a window that shows what the daemon knows and changes nothing. This crate is the bridge: it finds
//! the daemon, attaches on the read-only narrator scope, and gives the window a few named reads and the daemon's notifications.
//! When a terminal has paired it (spec 120), it attaches on the app socket instead, proving its key on every connection; the
//! window is given the same reads either way.

pub mod bridge;
pub mod client;
pub mod key;
pub mod locate;

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use bridge::{Bridge, Conn, Sink};
use client::Method;

struct Window(tauri::AppHandle);

impl Sink for Window {
    fn conn(&self, c: &Conn) {
        let _ = self.0.emit("delphi://conn", c);
    }
    fn events(&self, batch: Vec<Value>) {
        let _ = self.0.emit("delphi://events", batch);
    }
    fn frame(&self, frame: Value) {
        let _ = self.0.emit("delphi://frame", frame);
    }
    fn resync(&self) {
        let _ = self.0.emit("delphi://resync", ());
    }
}

/// Where the sockets are and how roles look: what `delphi daemon where` printed.
#[tauri::command]
fn info(bridge: State<'_, Arc<Bridge>>) -> Value {
    bridge.info()
}

#[tauri::command]
fn conn(bridge: State<'_, Arc<Bridge>>) -> Conn {
    bridge.conn()
}

/// `observe.state`: the daemon's folded picture of the current mission.
#[tauri::command(async)]
fn state(bridge: State<'_, Arc<Bridge>>) -> Result<Value, String> {
    bridge.call(Method::ObserveState, json!({}))
}

/// `observe.snapshot`: the newest persisted events of the kinds the daemon folds.
#[tauri::command(async)]
fn snapshot(bridge: State<'_, Arc<Bridge>>) -> Result<Value, String> {
    bridge.call(Method::ObserveSnapshot, json!({}))
}

/// `session.snapshot`: the daemon's mode, budget and which agents are on.
#[tauri::command(async)]
fn session(bridge: State<'_, Arc<Bridge>>) -> Result<Value, String> {
    bridge.call(Method::SessionSnapshot, json!({}))
}

/// One knowledge base read (`kb.get`): search, a listing or a document, and nothing else under `/v1/`.
#[tauri::command(async)]
fn kb(bridge: State<'_, Arc<Bridge>>, target: String) -> Result<Value, String> {
    if !bridge::kb_target_ok(&target) {
        return Err("that is not a knowledge base read this app makes".into());
    }
    bridge.call(Method::KbGet, json!({ "target": target }))
}

fn handed_over(done: std::io::Result<std::process::ExitStatus>, what: &str) -> Result<(), String> {
    match done {
        Ok(s) if s.success() => Ok(()),
        _ => Err(format!("{what} could not be opened")),
    }
}

fn system_open(target: &std::ffi::OsStr) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new(if cfg!(target_os = "macos") { "/usr/bin/open" } else { "xdg-open" }).arg(target).status()
}

/// Show a file in the Finder (spec 111 §2.6). The path is one the daemon listed or an agent named; it is shown only when it
/// is a file inside the workspace or a registered project once links are followed, and it is revealed, never opened or run.
#[tauri::command(async)]
fn reveal(bridge: State<'_, Arc<Bridge>>, path: String) -> Result<(), String> {
    let file = locate::find(&locate::roots(&bridge.info()), &path).ok_or("that is not a file in the workspace or a registered project")?;
    if cfg!(target_os = "macos") {
        handed_over(std::process::Command::new("/usr/bin/open").arg("-R").arg(&file).status(), "the file")
    } else {
        // Elsewhere: the folder that holds it.
        handed_over(system_open(file.parent().unwrap_or(&file).as_os_str()), "the folder")
    }
}

/// Open a deck, a page, a document, a picture or a recording in the system's browser or viewer (spec 111 §2.6). The same
/// confinement as `reveal`, and only for the kinds in `locate::OPENABLE`: never a program, a script or an archive.
#[tauri::command(async)]
fn open_file(bridge: State<'_, Arc<Bridge>>, path: String) -> Result<(), String> {
    let file = locate::find(&locate::roots(&bridge.info()), &path).ok_or("that is not a file in the workspace or a registered project")?;
    if !locate::openable(&file) {
        return Err("the app does not open that kind of file; show it in the Finder instead".into());
    }
    handed_over(system_open(file.as_os_str()), "the file")
}

/// Open a web address in the system's browser. Only `http` and `https`, and nothing that could be read as an option or a path.
#[tauri::command(async)]
fn open_url(url: String) -> Result<(), String> {
    if !web_address(&url) {
        return Err("only http and https addresses are opened".into());
    }
    handed_over(system_open(std::ffi::OsStr::new(&url)), "the address")
}

/// `http://` or `https://` followed by a host, with no space, control character or quote anywhere.
pub fn web_address(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    url.len() <= 2048 && rest.is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())) && !url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '`' | '\\' | '<' | '>'))
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let locator: bridge::Locator = Box::new(|| locate::locate().map(|l| (l.info, l.narrator)));
            // The key is kept per Delphi home (spec 120 §2 item 4): the keychain on macOS, a file only the operator reads elsewhere.
            let bridge = match locate::delphi_home() {
                Some(home) => Bridge::with_keys(env!("CARGO_PKG_VERSION"), locator, key::default_store(&home)),
                None => Bridge::new(env!("CARGO_PKG_VERSION"), locator),
            };
            app.manage(bridge.clone());
            let sink: Arc<dyn Sink> = Arc::new(Window(app.handle().clone()));
            std::thread::spawn(move || {
                // A terminal that started the app with `delphi app` left a pairing ticket on the standard input (a pipe); a
                // window started any other way has none.
                bridge.offer_ticket(key::ticket_from_stdin());
                bridge.run(sink)
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, conn, state, snapshot, session, kb, reveal, open_file, open_url])
        .run(tauri::generate_context!())
        .expect("the Delphi window could not start");
}

#[cfg(test)]
mod tests {
    use super::web_address;

    #[test]
    fn only_a_plain_web_address_is_handed_to_the_browser() {
        for ok in ["https://example.com", "http://example.com/a/b?c=d#e", "https://127.0.0.1:8080/x"] {
            assert!(web_address(ok), "{ok}");
        }
        for bad in ["file:///etc/hosts", "javascript:alert(1)", "https://", "https:// example.com", "-a https://example.com", "https://-flag", "ftp://example.com", "https://example.com/\n", "https://exa\"mple.com", "/Applications/Calculator.app", ""] {
            assert!(!web_address(bad), "{bad}");
        }
    }
}
