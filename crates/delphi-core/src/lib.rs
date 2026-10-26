//! `delphi-core`: the daemon's durable foundation (`specs/005` §2.2–§2.3).
//!
//! * [`event`]: the versioned `EventEnvelope` and its validation.
//! * [`redact`]: the deterministic redactor, including the signed-reasoning rules (`[H-003]`).
//! * [`log`]: the single-writer, segmented, fsync-before-act event log.
//! * [`ledger`]: the SQLite usage ledger, always rebuildable from the log (`specs/005` §2.4).
//! * [`budget`]: pre-dispatch reservations and the halt-on-ceiling rule (`specs/003` §3.2).

pub mod budget;
pub mod bundles;
pub mod client;
pub mod event;
pub mod hub;
pub mod ledger;
pub mod lock;
pub mod log;
pub mod pathguard;
pub mod peer;
pub mod provenance;
pub mod proto;
pub mod redact;
pub mod server;
pub mod spawn;
pub mod time;
pub mod totp;
pub mod usage;
#[cfg(any(test, feature = "test-support"))]
pub mod xenia_testkit;
pub mod xenia_store;
