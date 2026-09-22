//! `ingest daemon` (task 88, D-6/IN-1; schedule task 34, S11): the compose
//! `ingest` service mode. The worker checks the last successful scheduled
//! run at boot (design §6.1: a successful run for today is not duplicated,
//! an overdue day is caught up), then loops: ingest → sleep until the next
//! configured LOCAL run time (default 06:00 `America/Montevideo`,
//! `INGEST_TZ`/`INGEST_AT`) and repeat, forever. A failed pass is logged
//! and retried after the sleep — the daemon never exits on a failed run;
//! only a broken scheduler would justify dying.

use crate::commands::publish;
use crate::daily_loop::{self, CycleOutcome, SchedulerState};
use crate::exclusion::IngestionExclusion;
use crate::pool_config;
use crate::run_records;
use crate::support;
use db::pool;
use sqlx::types::chrono::Utc;
use std::path::Path;

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

    let mut scheduler = SchedulerState::fresh();
    let mut wake = daily_loop::restart_wake(last_success, Utc::now(), schedule.tz, schedule.at);
    eprintln!(
        "daemon: daily ingestion at {} {}",
        schedule.at.format("%H:%M"),
        schedule.tz.name()
    );
    let data_dir = std::env::var("TRAMITESUY_DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let base = std::env::var("CKAN_BASE_URL").unwrap_or_default();
    loop {
        sleep_until(wake);
        let outcome = scheduled_cycle(&pool, Path::new(&data_dir), &base, scheduler.attempt());
        let completed_at = Utc::now();
        wake = scheduler.step(outcome, completed_at, schedule.tz, schedule.at);
    }
}

/// One scheduled cycle: acquire the ingestion exclusion once around the
/// whole cycle (task 35), run the ingestion pass (download, process,
/// dual-write), and publish (build → validate → promote) with the
/// exclusion held. The outcome is what the daily scheduler steps on; the
/// cycle's run records carry the scheduler's attempt number (task 36).
pub fn scheduled_cycle(
    pool: &sqlx::PgPool,
    data_dir: &Path,
    base: &str,
    attempt: i16,
) -> CycleOutcome {
    scheduled_cycle_with(pool, data_dir, attempt, || {
        super::ingest::run_pass_on_pool(pool, base)
    })
}

/// One scheduled cycle over an INJECTED ingestion pass (the retry and
/// failure-injection tests drive the download directly; the production
/// path runs the real pipeline). The cycle's attempt number comes from the
/// scheduler.
pub fn scheduled_cycle_with(
    pool: &sqlx::PgPool,
    data_dir: &Path,
    attempt: i16,
    ingest_pass: impl FnOnce() -> Result<(), String>,
) -> CycleOutcome {
    // Phase A (shared runtime): the cycle's exclusion, acquired once around
    // ingest + publish — a held exclusion records this cycle `skipped` and
    // nothing is queued (the API keeps serving throughout).
    let held = support::block_on(async {
        match IngestionExclusion::try_acquire(pool).await {
            Err(error) => Err(error.to_string()),
            Ok(None) => Ok(None),
            Ok(Some(exclusion)) => Ok(Some((pool.clone(), exclusion))),
        }
    });
    let (cycle_pool, exclusion) = match held {
        Err(error) => {
            eprintln!("daemon: the ingestion exclusion check failed: {error}");
            return CycleOutcome::TransientFailure;
        }
        Ok(None) => {
            let recorded = support::block_on(async {
                run_records::record_terminal_run(
                    pool,
                    "scheduled",
                    "skipped",
                    serde_json::json!({ "reason": "ingestion exclusion held by another run" }),
                    attempt,
                )
                .await
            });
            if let Err(record_error) = recorded {
                eprintln!("daemon: the skipped run record failed: {record_error}");
            }
            eprintln!(
                "daemon: the cycle is skipped: the ingestion exclusion is held by another run"
            );
            return CycleOutcome::Skipped;
        }
        Ok(Some((cycle_pool, exclusion))) => (cycle_pool, exclusion),
    };

    // Phase B: the ingestion pass (download, process, dual-write) runs with
    // the exclusion held.
    let ingested = ingest_pass();

    // Phase C (shared runtime): publish with the exclusion held (build →
    // validate → promote); a failed download is recorded as the cycle's
    // failed run attempt. The exclusion releases on EVERY exit path —
    // commit, rollback, drop, or an unwind — so no path leaves a stuck
    // exclusion.
    support::block_on(async move {
        let outcome = match ingested {
            Ok(()) => match publish::publish_with_exclusion_held(
                &cycle_pool,
                data_dir,
                publish::Trigger::Scheduled,
                attempt,
            )
            .await
            {
                Ok(report) => {
                    eprintln!("daemon: publish {}", report.summary().trim());
                    match report.status.as_str() {
                        "success" => CycleOutcome::Completed,
                        // The exclusion is held by THIS cycle; a skipped
                        // report cannot occur — treated defensively as a
                        // failed cycle (the next attempt waits for the
                        // next scheduled run).
                        _ => CycleOutcome::TransientFailure,
                    }
                }
                Err(error) => {
                    eprintln!("daemon: the publish flow failed: {error}");
                    CycleOutcome::TransientFailure
                }
            },
            Err(error) => {
                let recorded = run_records::record_terminal_run(
                    &cycle_pool,
                    "scheduled",
                    "failed",
                    serde_json::json!({ "stage": "download", "error": error }),
                    attempt,
                )
                .await;
                if let Err(record_error) = recorded {
                    eprintln!("daemon: the failed run record could not be written: {record_error}");
                }
                eprintln!("daemon: the ingestion pass failed: {error}");
                CycleOutcome::TransientFailure
            }
        };
        drop(exclusion);
        outcome
    })
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
