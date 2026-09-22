//! Task 35 (S11, OPT-02, ingestion delta "Ingestion exclusion admits a
//! single active run"): scheduled AND manual runs share one PostgreSQL
//! advisory exclusion (`pg_advisory_lock(hashtext('tramitesuy:ingestion'))`).
//! A run that cannot acquire it terminates with a recorded `skipped` status
//! and is not queued, while the API keeps serving the pre-existing
//! generation throughout. TRIANGULATE: the exclusion is released on panic
//! and error paths — no stuck exclusion can wedge the installation.

mod common;

use common::*;
use ingest::exclusion::IngestionExclusion;
use ingest::support;

/// The repository's `data/` directory (real YAML taxonomy for the build).
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// The scratch URL for a created database name.
fn db_url(name: &str) -> String {
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    format!("{}/{}", base.trim_end_matches("/postgres"), name)
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

async fn newest_published(pool: &sqlx::PgPool) -> Option<uuid::Uuid> {
    db::generations::adopt::newest_published(pool)
        .await
        .expect("newest published")
        .map(|reference| reference.generation_id)
}

/// The manual run's thread runs the same composition the CLI runs, over
/// the explicit scratch database (no environment-variable races between
/// the parallel tests of this binary).
fn run_manual_ingest(base: &'static str, db_url: String) -> Result<(), String> {
    std::thread::spawn(move || ingest::commands::ingest::run_once_with_url(base, Some(&db_url)))
        .join()
        .expect("the manual ingest thread completes")
}

/// MANUAL overlapping SCHEDULED: the scheduled run holds the exclusion
/// (simulated by the test holding the same lock); the manual ingest does
/// not start processing — it records a `skipped` run and terminates, not
/// queued — while the published generation is untouched and the API keeps
/// serving it.
#[tokio::test(flavor = "multi_thread")]
async fn a_manual_run_overlapping_the_scheduled_run_is_skipped_and_recorded() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    let g1 = first.published_generation_id.expect("G1 published");

    // The API booted and adopted G1 (the pre-existing generation).
    let state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");
    assert_eq!(state.active.load_full().generation_id(), g1);

    // The scheduled run holds the ingestion exclusion on its own session.
    let scheduled_holds = IngestionExclusion::try_acquire(&pool)
        .await
        .expect("the scheduled run acquires the exclusion")
        .expect("the exclusion is free before the manual run");
    let _scheduled_holds = scheduled_holds;

    // The manual run cannot acquire the exclusion: it terminates with a
    // recorded `skipped` status and is NOT queued — it did not process the
    // (unreachable) source at all.
    let result = run_manual_ingest("http://127.0.0.1:1", db_url(&db_name));
    assert!(
        result.is_ok(),
        "a blocked manual run skips (recorded), it does not error: {result:?}"
    );

    // The skipped run is recorded: trigger `manual`, status `skipped`.
    let (trigger, status): (String, String) = sqlx::query_as(
        "SELECT trigger::text, status FROM ingestion_runs \
             ORDER BY started_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the skipped run record is read");
    assert_eq!(trigger, "manual", "the manual run is recorded");
    assert_eq!(status, "skipped", "the blocked run is recorded skipped");

    // The pre-existing generation was never touched: the API keeps serving
    // G1 throughout (the same snapshot, no swap, no new publication).
    assert_eq!(newest_published(&pool).await, Some(g1));
    assert_eq!(
        state.active.load_full().generation_id(),
        g1,
        "the API keeps serving the pre-existing generation throughout"
    );

    // Release the exclusion: a NEW manual run processes normally (it fails
    // at the unreachable source — the processing actually started this
    // time, the opposite of the blocked invocation above).
    drop(_scheduled_holds);
    let result = run_manual_ingest("http://127.0.0.1:1", db_url(&db_name));
    assert!(
        result.is_err(),
        "after the release the manual run starts processing (and fails at the unreachable source)"
    );

    common::drop_test_db(&db_name).await;
}

/// TRIANGULATE — no stuck exclusion: a run that PANICS while holding the
/// exclusion releases it (the transaction-scoped lock rolls back with the
/// guard), and the next run acquires it.
#[tokio::test(flavor = "multi_thread")]
async fn the_exclusion_releases_on_a_panic_path() {
    let (pool, db_name) = fresh_migrated_db().await;

    let pool_for_panic = pool.clone();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        support::block_on(async {
            let _held = IngestionExclusion::try_acquire(&pool_for_panic)
                .await
                .expect("the exclusion is acquirable")
                .expect("the exclusion is free");
            panic!("the run panics while holding the exclusion");
        });
    }));
    assert!(panicked.is_err(), "the run did panic");

    // The exclusion is free again: a fresh run acquires it (no stuck lock).
    let fresh = IngestionExclusion::try_acquire(&pool)
        .await
        .expect("the exclusion check succeeds")
        .expect("the panic released the exclusion — a fresh run acquires it");
    drop(fresh);

    common::drop_test_db(&db_name).await;
}

/// TRIANGULATE — no stuck exclusion on ERROR paths: a publish run that
/// fails mid-flow (after acquiring the exclusion) releases it; the next
/// run acquires it and succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn the_exclusion_releases_on_a_publish_error_path() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    assert!(first.published_generation_id.is_some(), "G1 publishes");

    // Break the source data the next build reads (the persistence layer's
    // dependency): the next publish errors mid-flow with the exclusion held.
    sqlx::query("DROP TABLE life_event_procedures")
        .execute(&pool)
        .await
        .expect("the relations table is dropped (injected persistence failure)");
    let failed = ingest::commands::publish::publish(
        &pool,
        &repo_data_dir(),
        ingest::commands::publish::Trigger::Manual,
    )
    .await;
    assert!(
        failed.is_err(),
        "the injected failure surfaces as a publish error"
    );

    // The exclusion was released on the error path (no stuck exclusion).
    let fresh = IngestionExclusion::try_acquire(&pool)
        .await
        .expect("the exclusion check succeeds")
        .expect("the error path released the exclusion — a fresh run acquires it");
    drop(fresh);

    common::drop_test_db(&db_name).await;
}
