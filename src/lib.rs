//! SiteWarden: Autonomous, ultra-lightweight browser smoke-testing daemon in Rust.
//!
//! Conforms to IEEE Std 830-1998 (SRS Specification).

pub mod alert;
pub mod browser;
pub mod config;
pub mod doctor;
pub mod engine;
pub mod pruner;
pub mod report;
pub mod scheduler;
pub mod state;
pub mod static_engine;
pub mod updater;
pub mod watcher;
