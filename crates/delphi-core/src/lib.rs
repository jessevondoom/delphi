//! `delphi-core`: the daemon's durable foundation (`specs/005` §2.2–§2.3).
//!
//! * [`event`]: the versioned `EventEnvelope` and its validation.
//! * [`redact`]: the deterministic redactor, including the signed-reasoning rules (`[H-003]`).
//! * [`log`]: the single-writer, segmented, fsync-before-act event log.

pub mod event;
pub mod log;
pub mod redact;
pub mod time;
