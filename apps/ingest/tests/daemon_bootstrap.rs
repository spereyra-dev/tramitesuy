//! F14 (WU-4): the production daemon bootstraps the taxonomy from an empty
//! database. Boot seeds the YAML taxonomy before the first ingestion pass
//! (so a fresh database has events and categories), each cycle re-seeds
//! after ingest (so relations whose procedure was not yet present resolve)
//! before publishing, and a taxonomy failure is a recorded failed cycle —
//! never a panic.

mod common;

use common::fresh_migrated_db;
use ingest::commands::daemon::{bootstrap_taxonomy, scheduled_cycle_seeded};
use ingest::daily_loop::CycleOutcome;
use sqlx::PgPool;
use std::path::{Path, PathBuf};

fn repo_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// (categories, events, relations) projected in the scratch database.
async fn projection_counts(pool: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM categories), \
                (SELECT count(*) FROM life_events), \
                (SELECT count(*) FROM life_event_procedures)",
    )
    .fetch_one(pool)
    .await
    .expect("projection counts")
}

/// One seeded-cycle execution on a plain thread (the cycle runs the
/// pipeline's blocking work off async workers).
fn run_seeded_cycle(
    pool: PgPool,
    data_dir: PathBuf,
    attempt: i16,
    ingest_pass: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> CycleOutcome {
    std::thread::spawn(move || scheduled_cycle_seeded(&pool, &data_dir, attempt, ingest_pass))
        .join()
        .expect("the scheduled cycle thread completes")
}

/// Bootstrap from an empty database seeds events and categories before any
/// ingestion; the first cycle then resolves the pending relations and
/// publishes successfully.
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_seeds_before_ingest_and_the_cycle_resolves_relations() {
    let (pool, db_name) = fresh_migrated_db().await;
    let data_dir = repo_data_dir();
    let snapshot = data_dir.join("external_ids.snapshot.txt");

    assert_eq!(
        projection_counts(&pool).await,
        (0, 0, 0),
        "the migrated database starts empty"
    );

    // Boot bootstrap: events and categories exist before the first pass.
    let report = bootstrap_taxonomy(&pool, &data_dir).expect("the bootstrap seeds the taxonomy");
    assert!(
        report.categories_inserted > 0 && report.events_inserted > 0,
        "the bootstrap inserts the YAML events and categories: {report:?}"
    );
    let (categories, events, relations) = projection_counts(&pool).await;
    assert_eq!(categories, 14, "all taxonomy categories are seeded at boot");
    assert_eq!(events, 104, "all taxonomy events are seeded at boot");
    assert_eq!(
        relations, 0,
        "relations stay pending until their procedure is ingested"
    );
    assert!(
        report.relations_pending > 0,
        "the boot seed reports the pending relations: {report:?}"
    );

    // First cycle: the injected pass ingests the catalog, then the cycle
    // re-seeds (resolving the pending relations) and publishes.
    let outcome = run_seeded_cycle(pool.clone(), data_dir, 1, {
        let pool = pool.clone();
        let snapshot = snapshot.clone();
        move || {
            ingest::support::block_on(common::seed_procedures_from_snapshot(&pool, &snapshot));
            Ok(())
        }
    });
    assert_eq!(
        outcome,
        CycleOutcome::Completed,
        "the first cycle completes: ingest → re-seed → publish"
    );

    let (_, _, relations) = projection_counts(&pool).await;
    assert!(
        relations > 0,
        "the per-cycle re-seed resolves the relations whose procedure was just ingested"
    );
    let published: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog_generations WHERE status = 'published'")
            .fetch_one(&pool)
            .await
            .expect("published generations");
    assert_eq!(published, 1, "the cycle publishes a generation");

    common::drop_test_db(&db_name).await;
}

/// A taxonomy failure inside a cycle is a recorded failed cycle, never a
/// panic: the cycle returns a transient failure and the run record names the
/// taxonomy stage.
#[tokio::test(flavor = "multi_thread")]
async fn a_taxonomy_failure_is_a_failed_cycle_not_a_panic() {
    let (pool, db_name) = fresh_migrated_db().await;
    let missing = PathBuf::from("/nonexistent-taxonomy-dir");

    let outcome = run_seeded_cycle(pool.clone(), missing, 1, || Ok(()));
    assert_eq!(
        outcome,
        CycleOutcome::TransientFailure,
        "a taxonomy load failure is reported as a failed cycle"
    );

    let (status, counts): (String, serde_json::Value) = sqlx::query_as(
        "SELECT status::text, counts FROM ingestion_runs ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the failed run is recorded");
    assert_eq!(status, "failed", "the cycle records a failed run");
    assert_eq!(
        counts.get("stage").and_then(|value| value.as_str()),
        Some("taxonomy"),
        "the run record names the taxonomy stage: {counts}"
    );

    common::drop_test_db(&db_name).await;
}
