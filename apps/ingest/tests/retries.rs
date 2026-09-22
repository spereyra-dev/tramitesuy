//! Task 36 (S11, OPT-03, ingestion delta "Bounded increasing retries, then
//! the next daily attempt"): transient ingestion failures (download,
//! validation, persistence) retry at +5, +15 and +30 minutes after the
//! initial failure; after exhausting the retries the failure is recorded
//! operationally and the next attempt waits for the next scheduled daily
//! run — while the previously published generation stays active
//! throughout. TRIANGULATE: a transient failure that succeeds on the
//! second attempt records no final failure.

mod common;

use chrono::TimeZone;
use chrono::Utc;
use common::*;
use ingest::commands::daemon::scheduled_cycle_with;
use ingest::daily_loop::{CycleOutcome, SchedulerState};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The repository's `data/` directory (real YAML taxonomy for the build).
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

async fn publish(pool: &sqlx::PgPool) -> ingest::commands::publish::PublishReport {
    ingest::commands::publish::publish(
        pool,
        &repo_data_dir(),
        ingest::commands::publish::Trigger::Manual,
    )
    .await
    .expect("publish runs")
}

/// One scheduled-cycle execution on a plain thread: the cycle runs the
/// pipeline's blocking HTTP work off async workers, with the ingestion
/// pass injected (the controllable failure).
fn run_cycle(
    pool: sqlx::PgPool,
    data_dir: std::path::PathBuf,
    attempt: i16,
    ingest_pass: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> CycleOutcome {
    std::thread::spawn(move || scheduled_cycle_with(&pool, &data_dir, attempt, ingest_pass))
        .join()
        .expect("the scheduled cycle thread completes")
}

/// The scheduled cycle's run records (trigger `scheduled`, started_at
/// order): the cycle records every execution with its attempt number.
async fn scheduled_run_records(pool: &sqlx::PgPool) -> Vec<(i16, String)> {
    sqlx::query_as(
        "SELECT attempt, status FROM ingestion_runs WHERE trigger = 'scheduled' \
         ORDER BY started_at",
    )
    .fetch_all(pool)
    .await
    .expect("scheduled run records read")
}

async fn newest_published(pool: &sqlx::PgPool) -> Option<uuid::Uuid> {
    db::generations::adopt::newest_published(pool)
        .await
        .expect("newest published")
        .map(|reference| reference.generation_id)
}

/// Seeds a small source catalog (taxonomy + procedures + relations).
async fn seed_source_catalog(pool: &sqlx::PgPool) {
    let taxonomy = taxonomy::loader::load_data_dir(&repo_data_dir()).expect("YAML taxonomy loads");
    db::repos::taxonomy_seed::seed_taxonomy(pool, &taxonomy)
        .await
        .expect("taxonomy seeds");

    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name) \
         VALUES ('D', 'Ministerio de ejemplo', 'ME')",
    )
    .execute(pool)
    .await
    .expect("seed organization");

    for (id, name) in [
        ("9001", "Solicitud de alta de vehículos"),
        ("9002", "Cambio de radicación"),
    ] {
        sqlx::query(
            "INSERT INTO procedures \
             (external_id, name, description, organization_id, official_url, status, raw_data) \
             VALUES ($1, $2, 'Descripcion del tramite', \
                     (SELECT id FROM organizations WHERE external_id = 'D'), \
                     $3, 'active', $4)",
        )
        .bind(id)
        .bind(name)
        .bind(format!("https://www.gub.uy/tramite/{id}"))
        .bind(serde_json::json!({ "id": id }))
        .execute(pool)
        .await
        .expect("seed procedure");
    }

    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, true \
         FROM life_events e, procedures p \
         WHERE e.slug = 'accidente-de-transito' AND p.external_id = '9001'",
    )
    .execute(pool)
    .await
    .expect("seed relation");
}

/// The daemon schedule (defaults): 06:00 America/Montevideo.
fn schedule() -> (chrono_tz::Tz, chrono::NaiveTime) {
    let config = ingest::daily_loop::ScheduleConfig::from_lookup(|_| None)
        .expect("the default schedule parses");
    (config.tz, config.at)
}

/// The always-failing injected download (the RED scenario).
fn failing_download() -> Result<(), String> {
    Err("injected download failure: the catalog source is unreachable".to_string())
}

/// A failing download at 06:00: exactly three retries at +5, +15 and +30
/// minutes (absolute offsets from the initial failure), `attempt` records
/// 1..3, then no further retry before the next day's 06:00 — and the
/// previously published generation stays active at every step.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_download_retries_on_the_bounded_schedule_then_waits_for_the_next_day() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let g1 = publish(&pool)
        .await
        .published_generation_id
        .expect("G1 published");

    // The API booted and adopted G1: the previously published generation.
    let api = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");
    assert_eq!(api.active.load_full().generation_id(), g1);

    let (tz, at) = schedule();
    // 06:00 America/Montevideo = 09:00 UTC (UTC-3 year-round).
    let run_at =
        |day: u32, h: u32, m: u32| Utc.with_ymd_and_hms(2026, 6, day, h + 3, m, 0).unwrap();
    let first_failure = run_at(1, 6, 0);

    let mut scheduler = SchedulerState::fresh();
    let data_dir = repo_data_dir();

    // Execution 1 at 06:00 (attempt 1): the download fails.
    let outcome = run_cycle(
        pool.clone(),
        data_dir.clone(),
        scheduler.attempt(),
        failing_download,
    );
    assert_eq!(outcome, CycleOutcome::TransientFailure);
    assert_eq!(
        scheduled_run_records(&pool).await,
        vec![(1, "failed".to_string())],
        "the initial failing execution is recorded attempt 1"
    );
    assert_eq!(newest_published(&pool).await, Some(g1));
    let wake = scheduler.step(outcome, first_failure, tz, at);
    assert_eq!(wake, first_failure + chrono::Duration::minutes(5));

    // Execution 2 at +5 (attempt 2): fails.
    let second_failure = run_at(1, 6, 5);
    let outcome = run_cycle(
        pool.clone(),
        data_dir.clone(),
        scheduler.attempt(),
        failing_download,
    );
    assert_eq!(outcome, CycleOutcome::TransientFailure);
    assert_eq!(
        scheduled_run_records(&pool).await,
        vec![(1, "failed".to_string()), (2, "failed".to_string())],
    );
    assert_eq!(newest_published(&pool).await, Some(g1));
    let wake = scheduler.step(outcome, second_failure, tz, at);
    assert_eq!(
        wake,
        run_at(1, 6, 15),
        "the retry waits +15 minutes after the +5-minute retry"
    );

    // Execution 3 at +15 (attempt 3): fails.
    let third_failure = run_at(1, 6, 15);
    let outcome = run_cycle(
        pool.clone(),
        data_dir.clone(),
        scheduler.attempt(),
        failing_download,
    );
    assert_eq!(outcome, CycleOutcome::TransientFailure);
    assert_eq!(
        scheduled_run_records(&pool).await,
        vec![
            (1, "failed".to_string()),
            (2, "failed".to_string()),
            (3, "failed".to_string()),
        ],
    );
    assert_eq!(newest_published(&pool).await, Some(g1));
    let wake = scheduler.step(outcome, third_failure, tz, at);
    assert_eq!(wake, run_at(1, 6, 30), "the last retry waits +30 minutes");

    // Execution 4 at +30 (the recorded attempt is capped at 3 — migration
    // 0014's `attempt BETWEEN 1 AND 3`): fails, the retries are exhausted:
    // the failure is recorded operationally and the next attempt waits for
    // the NEXT DAY's scheduled run.
    let fourth_failure = run_at(1, 6, 30);
    let outcome = run_cycle(
        pool.clone(),
        data_dir.clone(),
        scheduler.attempt(),
        failing_download,
    );
    assert_eq!(outcome, CycleOutcome::TransientFailure);
    assert_eq!(
        scheduled_run_records(&pool).await,
        vec![
            (1, "failed".to_string()),
            (2, "failed".to_string()),
            (3, "failed".to_string()),
            (3, "failed".to_string()),
        ],
        "the fourth execution records the capped attempt 3 (migration 0014 caps the column at 3)"
    );
    assert_eq!(newest_published(&pool).await, Some(g1));
    let wake = scheduler.step(outcome, fourth_failure, tz, at);
    assert_eq!(
        wake,
        run_at(2, 6, 0),
        "exhausted: the next attempt is TOMORROW's 06:00 run — no further retry before it"
    );

    // The scheduler is fresh: any instant of the rest of today sits before
    // the next wake (no retry fires).
    assert_eq!(scheduler.attempt(), 1);
    let late_today = run_at(1, 20, 0);
    assert!(
        wake > late_today,
        "no retry lands between the exhaustion and the next day's run"
    );

    common::drop_test_db(&db_name).await;
}

/// TRIANGULATE — a transient failure that SUCCEEDS on the second attempt:
/// no final failure is recorded; the scheduler's next wake is the next
/// day's run and the published generation is the succeeded one.
#[tokio::test(flavor = "multi_thread")]
async fn a_transient_failure_succeeding_on_the_second_attempt_records_no_final_failure() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let g1 = publish(&pool)
        .await
        .published_generation_id
        .expect("G1 published");

    let (tz, at) = schedule();
    let run_at =
        |day: u32, h: u32, m: u32| Utc.with_ymd_and_hms(2026, 6, day, h + 3, m, 0).unwrap();
    let first_failure = run_at(1, 6, 0);

    let mut scheduler = SchedulerState::fresh();
    let data_dir = repo_data_dir();

    // The ingestion pass fails the FIRST execution only; the second
    // attempt's pass succeeds and the publication flow runs for real
    // (identical content: the already-published generation, no new one).
    let calls = Arc::new(AtomicUsize::new(0));

    // Execution 1 at 06:00 (attempt 1): the pass fails at download.
    let outcome = run_cycle(pool.clone(), data_dir.clone(), scheduler.attempt(), {
        let calls = calls.clone();
        move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("injected download failure: the catalog source is unreachable".to_string())
            } else {
                Ok(())
            }
        }
    });
    assert_eq!(outcome, CycleOutcome::TransientFailure);
    let wake = scheduler.step(outcome, first_failure, tz, at);
    assert_eq!(wake, first_failure + chrono::Duration::minutes(5));

    // Execution 2 at +5 (attempt 2): the pass succeeds — the cycle
    // publishes (identical content: the already-published generation) and
    // the day's obligation is met.
    let second_instant = run_at(1, 6, 5);
    let outcome = run_cycle(
        pool.clone(),
        data_dir.clone(),
        scheduler.attempt(),
        move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("injected download failure".to_string())
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(outcome, CycleOutcome::Completed);
    let wake = scheduler.step(outcome, second_instant, tz, at);
    assert_eq!(
        wake,
        run_at(2, 6, 0),
        "a succeeded cycle waits for the NEXT day's scheduled run (no more retries)"
    );

    // The run records: the failed attempt 1 and the succeeded attempt 2 —
    // and NO final failure record after the success.
    let records = scheduled_run_records(&pool).await;
    assert_eq!(
        records,
        vec![(1, "failed".to_string()), (2, "success".to_string())],
        "the succeeded second attempt is recorded and no final failure follows"
    );
    assert_eq!(newest_published(&pool).await, Some(g1));

    common::drop_test_db(&db_name).await;
}
