//! Delphi desktop (spec 111): a window that shows what the daemon knows and changes nothing. This crate is the bridge: it finds
//! the daemon, attaches on the read-only narrator scope, and gives the window a few named reads and the daemon's notifications.

pub mod bridge;
pub mod client;
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

/// One knowledge base read (`kb.get`): search, a listing or a document, and nothing else under `/v1/`.
#[tauri::command(async)]
fn kb(bridge: State<'_, Arc<Bridge>>, target: String) -> Result<Value, String> {
    if !bridge::kb_target_ok(&target) {
        return Err("that is not a knowledge base read this app makes".into());
    }
    bridge.call(Method::KbGet, json!({ "target": target }))
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let bridge = Bridge::new(env!("CARGO_PKG_VERSION"), Box::new(|| locate::locate().map(|l| (l.info, l.narrator))));
            app.manage(bridge.clone());
            let sink: Arc<dyn Sink> = Arc::new(Window(app.handle().clone()));
            std::thread::spawn(move || bridge.run(sink));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, conn, state, snapshot, kb])
        .run(tauri::generate_context!())
        .expect("the Delphi window could not start");
}
