//! The record of what happens on the app socket (`specs/120` §3 item 4): each thing the daemon's server reports there
//! (`app.paired`, `app.refused`, `app.revoked`, `app.granted`, `app.expired`) is written as an event of the default
//! mission, where every attached client and `delphi app log` can read it. The server makes the payload from a key's short
//! name, a grant's name and a fixed reason: nothing a peer sent as text reaches the log.

use crate::service::EngineService;
use delphi_core::event::EventEnvelope;
use delphi_core::server::AppAudit;
use serde_json::Value;
use std::sync::Arc;

/// The event kinds the app socket writes. `app.request` is slice K4's.
pub const APP_EVENTS: [&str; 5] = ["app.paired", "app.refused", "app.revoked", "app.granted", "app.expired"];

impl EngineService {
    /// Write one app-socket record as an event. A kind outside [`APP_EVENTS`] is dropped: the server names the kind, and
    /// this is not a way to write any event.
    pub async fn record_app(&self, kind: &str, payload: Value) {
        if !APP_EVENTS.contains(&kind) {
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
        let refused = events_of(&f, "app.refused");
        assert_eq!(refused.len(), 1);
        assert_eq!((refused[0].payload["reason"].as_str(), refused[0].payload["name"].as_str()), (Some("bad_signature"), Some("app-0a1b2c")));
        assert!(events_of(&f, "approval.resolved").is_empty(), "the app record writes its own kinds only");
    }
}
