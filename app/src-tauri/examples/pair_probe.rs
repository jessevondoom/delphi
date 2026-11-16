//! `pair_probe`: what the app does when a terminal starts it, without a window (spec 120 §2). Name it in `DELPHI_APP` and
//! run `delphi app`: it reads the ticket from its standard input, pairs, opens the signed session on the app socket and
//! writes what happened to `<DELPHI_HOME>/app/pair-probe.txt`. Started with no ticket and a key from an earlier run, it shows
//! that the key alone opens a session. For checking a daemon and a bridge against each other on one machine.
//!
//! With `DELPHI_PROBE_ASK` set to a request as the window would name it (spec 120 §4.2), for example
//! `{"what":"page.rebuild","arg":{"slug":"tides"}}`, it also asks for that once the session is open and reports the
//! daemon's answer: the end-to-end check of spec 120 §9 without a window.
//!
//! It keeps its key in a file under `DELPHI_HOME`'s `app/` folder, never in the keychain, and refuses to run unless
//! `DELPHI_HOME` is set, so it cannot leave a key in the operator's real home by accident.

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use delphi_app_lib::bridge::{Bridge, Conn, Sink};
use delphi_app_lib::client::Method;
use delphi_app_lib::{key, locate};

struct Report(Mutex<mpsc::Sender<Conn>>);

impl Sink for Report {
    fn conn(&self, c: &Conn) {
        let _ = self.0.lock().unwrap().send(c.clone());
    }
    fn events(&self, _: Vec<Value>) {}
    fn frame(&self, _: Value) {}
    fn resync(&self) {}
}

fn main() {
    let Some(home) = std::env::var_os("DELPHI_HOME").map(std::path::PathBuf::from).filter(|h| h.is_absolute()) else {
        eprintln!("pair_probe: set DELPHI_HOME to the scratch home to pair in");
        std::process::exit(2);
    };
    let ticket = key::ticket_from_stdin();
    let bridge = Bridge::with_keys(env!("CARGO_PKG_VERSION"), Box::new(|| locate::locate().map(|l| (l.info, l.narrator))), Box::new(key::FileStore::under(&home)));
    bridge.offer_ticket(ticket.clone());
    let (tx, rx) = mpsc::channel();
    let (b, sink): (Arc<Bridge>, Arc<dyn Sink>) = (bridge.clone(), Arc::new(Report(Mutex::new(tx))));
    std::thread::spawn(move || b.run(sink));
    let mut said = json!({"ticket": ticket.is_some(), "connected": false});
    while let Ok(c) = rx.recv_timeout(Duration::from_secs(10)) {
        if c.connected {
            let ping = bridge.call(Method::Ping, json!({})).map_err(|e| e.to_string());
            said = json!({"ticket": ticket.is_some(), "connected": true, "detail": c.detail, "paired": c.paired, "grants": c.grants, "ping": format!("{ping:?}")});
            if let Some(ask) = std::env::var("DELPHI_PROBE_ASK").ok().and_then(|a| serde_json::from_str::<Value>(&a).ok()) {
                // Through the same two steps the window's command takes: the fields are picked and checked, then the bridge decides.
                let asked = match delphi_app_lib::ask::request(ask["what"].as_str().unwrap_or(""), &ask["arg"]) {
                    Some((method, params)) => bridge.request(method, params).map_err(|e| e.to_string()),
                    None => Err("not a request this app makes, or a field of another shape".to_string()),
                };
                said["asked"] = json!(format!("{asked:?}"));
            }
            break;
        }
        said["detail"] = json!(c.detail);
    }
    bridge.stop();
    let _ = std::fs::create_dir_all(home.join("app"));
    let _ = std::fs::write(home.join("app/pair-probe.txt"), format!("{said}\n"));
    println!("{said}");
}
