//! `ingest daemon` (task 88, D-6/IN-1): the compose `ingest` service mode —
//! runs one ingestion pass at boot, then sleeps until 03:00 UTC and repeats
//! daily, forever. A failed pass (missing config, transient source error)
//! is logged and retried after the sleep — the daemon never exits on a
//! failed run; only a broken scheduler would justify dying.

use std::time::Duration;

use crate::support;
use db::pool;
use ingest::daily_loop;

/// The daemon loop body. Runs migrations once (self-bootstrapping so the
/// compose service works on a fresh database), then loops: ingest → sleep
/// until the next 03:00 UTC.
pub fn run() {
    let pool = support::block_on(async {
        db::connect(&support::database_url(None))
            .await
            .expect("connect to Postgres")
    });
    support::block_on(async {
        pool::run_migrations(&pool)
            .await
            .expect("embedded migrations apply cleanly");
    });

    loop {
        match crate::commands::ingest::run_once() {
            Ok(()) => eprintln!("daemon: ingestion pass completed"),
            Err(message) => eprintln!("daemon: ingestion pass failed: {message}"),
        }
        let seconds = daily_loop::seconds_until_next_run(daily_loop::day_seconds_now());
        eprintln!("daemon: next ingestion run in {seconds} s (daily run at 03:00 UTC)");
        std::thread::sleep(Duration::from_secs(seconds));
    }
}
