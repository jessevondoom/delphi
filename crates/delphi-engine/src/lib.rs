//! `delphi-engine`: what runs inside the daemon: tools, approvals and the agent loop.

pub mod approval;
pub mod boot;
pub mod engine;
pub mod handoff;
pub mod mission;
pub mod model;
pub mod probe;
pub mod service;
pub mod tools;
pub mod workspace;

#[cfg(test)]
pub(crate) mod testkit;
