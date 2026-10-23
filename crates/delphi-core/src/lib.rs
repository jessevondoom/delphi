//! `delphi-core`: the daemon's durable foundation (`specs/005` §2.2–§2.3).
//!
//! * [`event`]: the versioned `EventEnvelope` and its validation.
//! * [`redact`]: the deterministic redactor, including the signed-reasoning rules (`[H-003]`).
//! * [`log`]: the single-writer, segmented, fsync-before-act event log.
//! * [`ledger`]: the SQLite usage ledger, always rebuildable from the log (`specs/005` §2.4).
//! * [`budget`]: pre-dispatch reservations and the halt-on-ceiling rule (`specs/003` §3.2).

pub mod budget;
pub mod event;
pub mod ledger;
pub mod log;
pub mod redact;
pub mod time;
pub mod usage;
