//! `delphi-engine`: what runs inside the daemon: tools, approvals and the agent loop.

pub mod approval;
pub mod auth_cli;
pub mod boot;
pub mod core_main;
pub mod build;
pub mod capabilities;
pub mod endpoints_cli;
pub mod engine;
pub mod exec;
pub mod finalize;
pub mod gates;
pub mod handoff;
pub mod harness;
pub mod harness_run;
pub mod mission;
pub mod model;
pub mod probe;
pub mod recovery;
pub mod ratify;
pub mod service;
pub mod status;
pub mod tools;
pub mod workspace;
pub mod worktree;

#[cfg(test)]
pub(crate) mod testkit;
