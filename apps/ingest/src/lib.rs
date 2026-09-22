//! The `ingest` library surface: everything the binary and the integration
//! tests share. The daemon's daily-loop scheduling math lives here as a
//! pure, testable function (task 88); the promotion flow (S6 task 18) lives
//! in `commands::publish` so tests compose it directly.

pub mod commands;
pub mod daily_loop;
pub mod errors;
pub mod exclusion;
pub mod pool_config;
pub mod reconciliation;
pub mod run_records;
pub mod support;
