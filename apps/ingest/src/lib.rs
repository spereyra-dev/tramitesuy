//! The `ingest` library surface: everything the binary and the integration
//! tests share. The daemon's daily-loop scheduling math lives here as a
//! pure, testable function (task 88).

pub mod daily_loop;
pub mod pool_config;
