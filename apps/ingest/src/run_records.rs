//! Durable run-record reads and terminal writes for the worker (S11,
//! design §6.1/§6.3): the restart gate reads the last successful scheduled
//! run so a worker restart neither duplicates the day's scheduled
//! ingestion nor leaves an overdue day unrun; the flows that never start
//! work (excluded runs) record a terminal row (`skipped`, not queued).
//! Build/promotion run records stay with `commands::publish`.

use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

/// The start instant of the most recent successful scheduled run, if any
/// (the restart gate's "did today's run already succeed" input).
pub async fn last_succeeded_scheduled_started_at(
    pool: &PgPool,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT max(started_at) AS \"max: Option<DateTime<Utc>>\" FROM ingestion_runs \
         WHERE trigger = 'scheduled' AND status = 'success'",
    )
    .fetch_one(pool)
    .await
    .map(|max| max.flatten())
}

/// Records one terminal run row for a flow that never started work: an
/// excluded run is recorded `skipped` (not queued), with the trigger,
/// attempt, and the reason in `counts`.
pub async fn record_terminal_run(
    pool: &PgPool,
    trigger: &str,
    status: &str,
    counts: serde_json::Value,
    attempt: i16,
) -> Result<Uuid, sqlx::Error> {
    let run_id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO ingestion_runs \
         (run_id, trigger, started_at, finished_at, status, counts, \
          candidate_generation_id, published_generation_id, attempt) \
         VALUES ($1, $2, now(), now(), $3, $4, NULL, NULL, $5)",
        run_id,
        trigger,
        status,
        counts,
        attempt,
    )
    .execute(pool)
    .await?;
    Ok(run_id)
}
