//! `ingest daemon` (task 88, D-6/IN-1): the compose `ingest` service mode —
//! runs one ingestion pass at boot, then sleeps until 03:00 UTC and repeats
//! daily, forever. A failed pass (missing config, transient source error)
//! is logged and retried after the sleep — the daemon never exits on a
//! failed run; only a broken scheduler would justify dying.

use std::time::Duration;

use crate::daily_loop;
use crate::pool_config;
use crate::support;
use db::pool;

/// The daemon loop body. Runs migrations once (self-bootstrapping so the
/// compose service works on a fresh database), then loops: ingest → sleep
/// until the next 03:00 UTC.
pub fn run() {
    let limits = pool_config::PoolLimits::from_env().unwrap_or_else(|error| {
        // Panic justification: boot composition root of the daemon; an
        // invalid environment is a fatal boot failure (compose restarts it)
        // rather than looping forever on a broken configuration.
        panic!("daemon pool config: {error}")
    });
    let pool = support::block_on(async {
        db::connect(
            &support::database_url(None),
            limits.pool_max,
            limits.acquire_timeout,
        )
        .await
        // Panic justification: boot composition root of the daemon; a
        // usable database is part of the daemon's environment contract and
        // a failed boot aborts the process (compose restarts it) rather
        // than looping forever on a broken pool.
        .expect("connect to Postgres")
    });
    support::block_on(async {
        // Panic justification: same boot contract as above — migrations
        // must apply before the daemon can do any work.
        pool::run_migrations(&pool)
            .await
            .expect("embedded migrations apply cleanly");
    });

    // Manifest reconciliation (S8 task 23): a background thread confirms
    // the API's adoption of publications, alerts on the lagging bound, and
    // runs the gated retention collection — off the request path, never
    // touching the active or previous generation.
    let reconcile_pool = pool.clone();
    let reconcile_config =
        crate::reconciliation::ReconcileConfig::from_env().unwrap_or_else(|error| {
            // Panic justification: boot composition root of the daemon; an
            // invalid environment is a fatal boot failure (compose restarts
            // it) rather than reconciling with a policy the operator did
            // not choose.
            panic!("daemon reconciliation config: {error}")
        });
    std::thread::spawn(move || {
        loop {
            let outcome = support::block_on(async {
                crate::reconciliation::run_pass(&reconcile_pool, &reconcile_config).await
            });
            match outcome {
                Ok(report) => {
                    eprintln!(
                        "daemon reconciliation: {}",
                        crate::reconciliation::summarize(&report)
                    );
                }
                Err(error) => {
                    eprintln!("daemon reconciliation pass failed: {error}");
                }
            }
            std::thread::sleep(reconcile_config.interval);
        }
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
