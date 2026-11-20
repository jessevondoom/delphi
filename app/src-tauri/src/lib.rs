//! Delphi desktop (spec 111): a window that shows what the daemon knows and changes nothing. This crate is the bridge: it finds
//! the daemon, attaches on the read-only narrator scope, and gives the window a few named reads and the daemon's notifications.
//! When a terminal has paired it (spec 120), it attaches on the app socket instead, proving its key on every connection; the
//! window is given the same reads either way, and a paired window the encyclopedia's reads as well (spec 117 §3; `ency`).
//! A paired window whose key holds `request` may also ask for four things to be started (spec 120 §4.2; `ask`).
//! The operator's own marks on the encyclopedia (spec 117 §5; `marks`) are kept in the app's folder and never sent anywhere.

pub mod ask;
pub mod bridge;
pub mod client;
pub mod ency;
pub mod key;
pub mod locate;
pub mod marks;

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

/// One knowledge base read (`kb.get`): search, a listing, a document or a picture beside one, and nothing else under
/// `/v1/`. A picture's size is clamped here as well as by the daemon (`bridge::kb_target`).
#[tauri::command(async)]
fn kb(bridge: State<'_, Arc<Bridge>>, target: String) -> Result<Value, String> {
    let Some(target) = bridge::kb_target(&target) else {
        return Err("that is not a knowledge base read this app makes".into());
    };
    bridge.call(Method::KbGet, json!({ "target": target }))
}

/// One of the encyclopedia's eleven reads (spec 117 §3 item 1): topic pages, their versions and links, research topics,
/// one source, one claim, agent memory, what changed. The window names the read and its arguments; `ency::read` picks the
/// method and builds the parameters. Sent only when this app proved a paired key (spec 120): otherwise the answer is the
/// sentence that says so, and the daemon is not asked.
#[tauri::command(async)]
fn encyclopedia(bridge: State<'_, Arc<Bridge>>, read: String, arg: Option<Value>) -> Result<Value, String> {
    let arg = arg.unwrap_or(Value::Null);
    let Some((method, params)) = ency::read(&read, &arg) else {
        return Err("that is not an encyclopedia read this app makes".into());
    };
    bridge.call(method, params).map(|reply| ency::shown(method, &arg, reply))
}

/// One of the four requests of the daemon's `request` grant (spec 120 §4.2, §6 item 1): continue a research topic, ask a
/// follow-up question, rebuild a topic page, dispute a claim. The window names the request and hands over ids and what
/// the operator typed; `ask::request` picks the method and builds the parameters from those fields alone. Sent only when
/// this app proved a paired key that holds `request`: otherwise the answer is the sentence that says so, and the daemon
/// is not asked.
#[tauri::command(async)]
fn request(bridge: State<'_, Arc<Bridge>>, what: String, arg: Option<Value>) -> Result<Value, String> {
    let Some((method, params)) = ask::request(&what, &arg.unwrap_or(Value::Null)) else {
        return Err("that is not a request this app makes, or one of its fields is not of the shape it takes".into());
    };
    bridge.request(method, params)
}

/// What the start screen is drawn from (spec 130 §4): the daemons found in the run folder of the Delphi home, whether the
/// bridge is waiting for one to be chosen, and which one it is attached to.
#[derive(serde::Serialize)]
struct Daemons {
    held: bool,
    current: Option<String>,
    /// The app's own version (`0.0.0` in a build that was not made for a release), for a card that says the daemon's differs.
    app_version: String,
    daemons: Vec<locate::Found>,
    /// Whether an installed `delphi` exists, and this app's bundled one (spec 122 §10.1 item 3), for the empty state.
    program: locate::Program,
}

/// The daemons on this machine (spec 130 §3.1), each with a summary read through `observe.state` when `summaries` is asked
/// for (the start screen and the switcher ask while they are showing, and no daemon is read more than every five seconds).
/// It reads files and sockets of the Delphi home and starts nothing.
#[tauri::command(async)]
fn daemons(app: tauri::AppHandle, bridge: State<'_, Arc<Bridge>>, summaries: Option<bool>) -> Result<Daemons, String> {
    let daemons = bridge.daemons(&home()?, summaries.unwrap_or(false));
    Ok(Daemons { held: bridge.held(), current: bridge.current(), app_version: app.package_info().version.to_string(), daemons, program: locate::program() })
}

/// Point the window at one daemon of the list `daemons` last gave (spec 130 §3.2): the bridge attaches to it again. Any other
/// hash is refused and nothing changes.
#[tauri::command(async)]
fn attach(bridge: State<'_, Arc<Bridge>>, hash: String) -> Result<(), String> {
    bridge.attach_to(&hash, locate::info_for)
}

fn home() -> Result<std::path::PathBuf, String> {
    locate::delphi_home().ok_or_else(|| "no Delphi home was found on this machine".to_string())
}

/// The operator's own marks (spec 117 §5): read pages, notes on passages, pins. Kept in `<home>/app/marks.json` and never
/// sent to the daemon; reading them creates nothing.
#[tauri::command(async)]
fn marks_load() -> Result<Value, String> {
    marks::load(&home()?)
}

#[tauri::command(async)]
fn marks_save(marks: Value) -> Result<(), String> {
    marks::save(&home()?, &marks)
}

/// Export a page as Markdown (spec 117 §5 item 4): written to `<home>/app/exports/<name>.md` with its banner, and shown
/// in the Finder. Returns where it was written.
#[tauri::command(async)]
fn export_page(name: String, text: String) -> Result<String, String> {
    let file = marks::export(&home()?, &name, &text)?;
    // Shown, never opened or run; a system that cannot show it still has the file, and the window says where.
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(&file).status();
    }
    Ok(file.to_string_lossy().into_owned())
}

/// The system's print dialog for the page in the window (spec 117 §5 item 4): the operator prints it or saves it as PDF.
#[tauri::command]
fn print_page(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| format!("the print dialog could not be opened: {e}"))
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
                // Which daemon to show (spec 130 §3.2): the one that runs, or the one `DELPHI_WORKSPACE` names, as before; with
                // several and no name, none until the start screen's choice.
                if let Some(home) = locate::delphi_home() {
                    let found = bridge.daemons(&home, false);
                    let named = std::env::var("DELPHI_WORKSPACE").ok();
                    match locate::start_choice(&found, named.as_deref()) {
                        locate::Start::Ask => bridge.hold(),
                        locate::Start::Attach(hash) => {
                            let _ = bridge.attach_to(&hash, locate::info_for);
                        }
                        locate::Start::Locate => {}
                    }
                }
                // A terminal that started the app with `delphi app` left a pairing ticket on the standard input (a pipe); a
                // window started any other way has none.
                bridge.offer_ticket(key::ticket_from_stdin());
                bridge.run(sink)
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, conn, state, snapshot, session, daemons, attach, kb, encyclopedia, request, marks_load, marks_save, export_page, print_page, reveal, open_file, open_url])
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
