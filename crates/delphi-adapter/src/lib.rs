//! `delphi-adapter`: how Delphi reaches models (`specs/009`, `specs/002`).

pub mod blocks;
pub mod claude;
pub mod config;
pub mod credential;
pub mod eventstream;
pub mod http;
pub mod ipr;
pub mod platform;
pub mod sigv4;
pub use reqwest;
