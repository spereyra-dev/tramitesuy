//! `ingest daemon` (task 88, D-6/IN-1; schedule task 34, S11): the compose
//! `ingest` service mode. The worker checks the last successful scheduled
//! run at boot (design §6.1: a successful run for today is not duplicated,
//! an overdue day is caught up), then loops: ingest → sleep until the next
//! configured LOCAL run time (default 06:00 `America/Montevideo`,
//! `INGEST_TZ`/`INGEST_AT`) and repeat, forever. A failed pass is logged
//! and retried after the sleep — the daemon never exits on a failed run;
//! only a broken scheduler would justify dying.

use crate::daily_loop;
use crate::pool_config;
use crate::run_records;
use crate::support;
use db::pool;
use sqlx::types::chrono::Utc;

/// The daemon loop body. Runs migrations once (self-bootstrapping so the
/// compose service works on a fresh database), then loops: ingest → sleep
/// until the next scheduled local run.
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

    let schedule = daily_loop::ScheduleConfig::from_env().unwrap_or_else(|error| {
        // Panic justification: boot composition root of the daemon; an
        // invalid schedule configuration is a fatal boot failure (compose
        // restarts it) rather than looping forever on a schedule the
        // operator did not choose.
        panic!("daemon schedule config: {error}")
    });

    // Restart gate (design §6.1): the last successful scheduled run decides
    // whether the worker catches up at boot or waits for the next run.
    let last_success = support::block_on(async {
        run_records::last_succeeded_scheduled_started_at(&pool)
            .await
            // Panic justification: boot composition root; the restart gate
            // cannot decide without its run-record read.
            .expect("the last successful scheduled run is readable")
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

    let mut wake = daily_loop::restart_wake(last_success, Utc::now(), schedule.tz, schedule.at);
    eprintln!(
        "daemon: daily ingestion at {} {}",
        schedule.at.format("%H:%M"),
        schedule.tz.name()
    );
    loop {
        sleep_until(wake);
        let succeeded = match crate::commands::ingest::run_once() {
            Ok(()) => {
                eprintln!("daemon: ingestion pass completed");
                true
            }
            Err(message) => {
                eprintln!("daemon: ingestion pass failed: {message}");
                false
            }
        };
        let now = Utc::now();
        // A successful pass covers the local day: the next run is the next
        // day's scheduled time (never a duplicate the same day). A failed
        // pass is retried at the next scheduled instant (possibly today).
        wake = if succeeded {
            daily_loop::next_day_run(now, schedule.tz, schedule.at)
        } else {
            daily_loop::next_run(now, schedule.tz, schedule.at)
        };
    }
}

/// Sleeps (blocking) until the absolute wake instant.
fn sleep_until(wake: sqlx::types::chrono::DateTime<Utc>) {
    loop {
        let now = Utc::now();
        if wake <= now {
            return;
        }
        // Justified: the difference is positive in this branch; a clock
        // running backwards during the sleep re-loops (the daemon treats a
        // negative remaining time as "wake now").
        let remaining = (wake - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(1));
        std::thread::sleep(remaining);
    }
}
