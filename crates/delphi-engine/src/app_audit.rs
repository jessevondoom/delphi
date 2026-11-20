//! The record of what happens on the app socket (`specs/120` §3 item 4): each thing the daemon's server reports there
//! (`app.paired`, `app.refused`, `app.revoked`, `app.granted`, `app.expired`, and, since spec 120 §13.1, `app.pair_requested` and `app.pair_approved`) is written as an event of the default
//! mission, where every attached client and `delphi app log` can read it. The server makes the payload from a key's short
//! name, a grant's name and a fixed reason: nothing a peer sent as text reaches the log.
//!
//! The sixth kind, `app.request` (§4.2 item 4), is not the server's to report: the engine writes it itself for each request
//! it counted ([`crate::service::app_requests`]), and the server's sink refuses it.

use crate::service::EngineService;
use delphi_core::event::EventEnvelope;
use delphi_core::server::AppAudit;
use serde_json::Value;
use std::sync::Arc;

/// The event kinds the app socket writes.
pub const APP_EVENTS: [&str; 10] = ["app.paired", "app.refused", "app.revoked", "app.granted", "app.expired", REQUEST_EVENT, "app.pair_requested", "app.pair_approved", UPDATED_EVENT, UPDATE_REFUSED_EVENT];
/// A request of the `request` grant and each change of its state (`specs/120` §4.2 item 4).
pub const REQUEST_EVENT: &str = "app.request";
/// An update the daemon applied for a key, with the two versions, and one it refused or that failed, with the reason (`specs/120` §13.2
/// item 9). Like [`REQUEST_EVENT`], written by the engine itself and never by the server's sink.
pub const UPDATED_EVENT: &str = "app.updated";
pub const UPDATE_REFUSED_EVENT: &str = "app.update_refused";

impl EngineService {
    /// Write one app-socket record as an event. A kind outside [`APP_EVENTS`] is dropped: the server names the kind, and
    /// this is not a way to write any event.
    pub async fn record_app(&self, kind: &str, payload: Value) {
        if !APP_EVENTS.contains(&kind) || matches!(kind, REQUEST_EVENT | UPDATED_EVENT | UPDATE_REFUSED_EVENT) {
            return;
        }
        if let Ok(m) = self.mission_at(None, None).await {
            let _ = m.append(EventEnvelope::new(&m.id, kind, payload)).await;
        }
    }

    /// What the daemon's server calls ([`delphi_core::server::Daemon::set_app_audit`]). It is called from the server's tasks
    /// and must not block them, so the write is its own task.
    pub fn app_audit(self: &Arc<Self>) -> AppAudit {
        let svc = self.clone();
        Arc::new(move |kind: &str, payload: Value| {
            let (svc, kind) = (svc.clone(), kind.to_string());
            tokio::spawn(async move { svc.record_app(&kind, payload).await });
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::service::EngineService;
    use crate::testkit::{events_of, setup};
    use serde_json::json;

    /// AC-4's second half, at the engine: a refusal the server reports is an `app.refused` event in the log.
    #[tokio::test]
    async fn a_refusal_on_the_app_socket_is_written_as_an_event_and_no_other_kind_can_be() {
        let f = setup(vec![], 5.0, |_| {});
        let svc = EngineService::new(f.engine.clone(), "m1");
        svc.adopt(f.m.clone());
        svc.record_app("app.refused", json!({"reason": "bad_signature", "name": "app-0a1b2c", "locked_out": false})).await;
        svc.record_app("approval.resolved", json!({"approval_id": "ap_1", "resolution": "allow"})).await;
        // A request's record is the engine's own to write, for a request it counted: the server cannot report one.
        svc.record_app("app.request", json!({"name": "app-0a1b2c", "method": "app.page_rebuild", "ids": {"slug": "tides"}, "state": "done"})).await;
        assert!(events_of(&f, "app.request").is_empty());
        assert_eq!(super::APP_EVENTS.len(), 10);
        // The update outcomes are the engine's own to write, as a request's state is.
        svc.record_app("app.updated", json!({"name": "app-0a1b2c", "from": "0.6.5", "to": "0.6.6"})).await;
        svc.record_app("app.update_refused", json!({"name": "app-0a1b2c", "reason": "none_available"})).await;
        assert!(events_of(&f, "app.updated").is_empty() && events_of(&f, "app.update_refused").is_empty());
        let refused = events_of(&f, "app.refused");
        assert_eq!(refused.len(), 1);
        assert_eq!((refused[0].payload["reason"].as_str(), refused[0].payload["name"].as_str()), (Some("bad_signature"), Some("app-0a1b2c")));
        assert!(events_of(&f, "approval.resolved").is_empty(), "the app record writes its own kinds only");
    }
}
