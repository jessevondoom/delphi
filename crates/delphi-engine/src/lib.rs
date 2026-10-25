//! `delphi-engine`: what runs inside the daemon: tools, approvals and the agent loop.

pub mod approval;
pub mod auth_cli;
pub mod boot;
pub mod core_main;
pub mod council;
pub mod build;
pub mod capabilities;
pub mod connector_run;
pub mod connectors;
pub mod connectors_cli;
pub mod endpoints_cli;
pub mod engine;
pub mod exec;
pub mod finalize;
pub mod gates;
pub mod handoff;
pub mod harness;
pub mod harness_run;
pub mod embed;
pub mod kb;
pub mod vecext;
pub mod mission;
pub mod models_cli;
pub mod model;
pub mod observe;
pub mod pool;
pub mod probe;
pub mod provisional;
pub mod recovery;
pub mod route;
pub mod ratify;
pub mod service;
pub mod status;
pub mod tools;
pub mod workspace;
pub mod worktree;

#[cfg(test)]
pub(crate) mod testkit;
