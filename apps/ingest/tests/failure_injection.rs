//! S8 task 26 (catalog-generations delta "Previous generation remains
//! recoverable", spec §7 test 3, R5/R13): the failure-injection matrix
//! BEFORE cache activation. Failures are injected into the download,
//! validation, persistence, and promotion phases — with worker/API restarts
//! between phases (fresh pools/process-equivalent connections). In every
//! injected failure the PREVIOUS version stays active, the run records
//! reflect the state, and nothing is ever repaired by mutating the
//! defective candidate.

mod common;

use common::*;

use ingest::commands::publish::Trigger;
use sqlx::Row;
use uuid::Uuid;

/// The repository's `data/` directory (real YAML taxonomy for the build).
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
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

/// Publishes one generation from the current legacy content (the worker's
/// promotion flow, task 18).
async fn publish(pool: &sqlx::PgPool) -> ingest::commands::publish::PublishReport {
    ingest::commands::publish::publish(pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish runs")
}

async fn manifest_rows(pool: &sqlx::PgPool) -> Vec<(Uuid, String)> {
    sqlx::query("SELECT generation_id, status::text FROM catalog_generations ORDER BY created_at")
        .map(|row: sqlx::postgres::PgRow| (row.get::<uuid::Uuid, _>(0), row.get::<String, _>(1)))
        .fetch_all(pool)
        .await
        .expect("manifest rows")
}

async fn newest_published(pool: &sqlx::PgPool) -> Option<Uuid> {
    db::generations::adopt::newest_published(pool)
        .await
        .expect("newest published")
        .map(|reference| reference.generation_id)
}

async fn adoption_of(pool: &sqlx::PgPool, generation: Uuid) -> bool {
    let (active, adopted): (Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT active_generation_id, adopted_at::text \
         FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(generation)
    .fetch_one(pool)
    .await
    .expect("manifest row");
    active == Some(generation) && adopted.is_some()
}

async fn run_records(pool: &sqlx::PgPool) -> Vec<(String, Option<String>)> {
    sqlx::query("SELECT status::text, finished_at::text FROM ingestion_runs ORDER BY started_at")
        .map(|row: sqlx::postgres::PgRow| {
            (row.get::<String, _>(0), row.get::<Option<String>, _>(1))
        })
        .fetch_all(pool)
        .await
        .expect("run records")
}

/// A "worker restart": a brand-new pool over the same scratch database
/// (process-equivalent for the phases' connection/state isolation).
async fn restarted_pool(name: &str) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url(name))
        .await
        .expect("worker restart connects a fresh pool")
}

/// The scratch URL for a created database name.
fn db_url(name: &str) -> String {
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    format!("{}/{}", base.trim_end_matches("/postgres"), name)
}

/// DOWNLOAD failure: the source fetch fails before anything is built or
/// published; the previous version stays active and no manifest row, no
/// run record appears (the ingestion pass never reaches the publication
/// flow).
#[tokio::test(flavor = "multi_thread")]
async fn download_failure_keeps_the_previous_generation_active() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    let g1 = first.published_generation_id.expect("G1 published");

    // The API (fresh boot = API restart) adopted G1.
    let state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");
    assert_eq!(state.active.load_full().generation_id(), g1);

    // Download failure: the catalog source is unreachable (injected via the
    // explicit base URL; the production path reads CKAN_BASE_URL). The
    // pass runs on its own thread (the pipeline owns its runtime; a
    // blocking reqwest client must never be built on an async worker).
    let result =
        std::thread::spawn(|| ingest::commands::ingest::run_once_with_base("http://127.0.0.1:1"))
            .join()
            .expect("the failing pass thread completes with its error");
    assert!(
        result.is_err(),
        "the download failure surfaces as the run's error"
    );

    // The previous version stays active: no new manifest row, no adoption
    // change, and no NEW run record from the failed pass (the download
    // failure happens before any publication run is opened).
    let runs_before: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion_runs")
        .fetch_one(&pool)
        .await
        .expect("run count before");
    assert_eq!(
        manifest_rows(&pool).await.len(),
        1,
        "no new generation was built"
    );
    assert_eq!(newest_published(&pool).await, Some(g1));
    assert!(
        adoption_of(&pool, g1).await,
        "the previous adoption record stands"
    );
    let result =
        std::thread::spawn(|| ingest::commands::ingest::run_once_with_base("http://127.0.0.1:1"))
            .join()
            .expect("the failing pass thread completes with its error");
    assert!(
        result.is_err(),
        "the download failure surfaces as the run's error"
    );
    let runs_after: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion_runs")
        .fetch_one(&pool)
        .await
        .expect("run count after");
    assert_eq!(
        runs_after, runs_before,
        "no run record from the failed download pass"
    );
    // A restarted API still serves G1.
    let state_after = api::state::AppState::boot(
        restarted_pool(&db_name).await,
        &repo_data_dir(),
        Default::default(),
    )
    .await
    .expect("API restart boots G1");
    assert_eq!(state_after.active.load_full().generation_id(), g1);

    drop_test_db(&db_name).await;
}

/// VALIDATION failure: a dangling relation is captured into the candidate
/// build; the validation gate rejects it, the run record reflects
/// `validation_failed`, and the previous version stays active through an
/// API restart.
#[tokio::test(flavor = "multi_thread")]
async fn validation_failure_keeps_the_previous_version_active() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    let g1 = first.published_generation_id.expect("G1 published");
    let _state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");

    // Content change builds a validated candidate; then the candidate's
    // projected details are corrupted (the artifact loss that validation
    // exists for): its cards would reference procedures with no projected
    // details — dangling relations.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '9001'")
        .execute(&pool)
        .await
        .expect("content change");
    let taxonomy_version =
        ingest::support::compute_taxonomy_version(&repo_data_dir()).expect("taxonomy version");
    let built = db::generations::build::build_generation(&pool, &taxonomy_version)
        .await
        .expect("candidate builds");
    let g2 = built.generation_id;
    let taxonomy = taxonomy::loader::load_data_dir(&repo_data_dir()).expect("taxonomy loads");
    let gate = db::generations::validate::validate_generation(&pool, g2, Some(&taxonomy))
        .await
        .expect("validation runs");
    assert!(
        gate.passed(),
        "the candidate validates before the corruption: {:?}",
        gate.failures
    );
    sqlx::query("DELETE FROM generation_procedure_details WHERE generation_id = $1")
        .bind(g2)
        .execute(&pool)
        .await
        .expect("corrupt the candidate's projected details");
    // The corrupted candidate is rejected as a publication candidate, and
    // the gate reports every applicable failure: the missing active procedure
    // AND the dangling relations, in that order, instead of short-circuiting.
    let gate_after = db::generations::validate::validate_generation(&pool, g2, Some(&taxonomy))
        .await
        .expect("validation after corruption");
    let failure_kinds: Vec<&str> = gate_after
        .failures
        .iter()
        .map(|failure| failure.kind)
        .collect();
    assert_eq!(
        failure_kinds,
        vec!["empty_active_catalog", "relation_integrity"],
        "the corrupted candidate reports every applicable failure: {:?}",
        gate_after.failures
    );

    // Worker restart: a fresh pool runs the flow.
    let fresh = restarted_pool(&db_name).await;
    let report = ingest::commands::publish::publish(&fresh, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("the flow runs and reports the failure");
    assert_eq!(
        report.status, "validation_failed",
        "the validation gate rejects the defective candidate: {report:?}"
    );
    assert!(
        !report.validation_failures.is_empty(),
        "the failures are recorded on the report"
    );

    // Run records reflect the state.
    let records = run_records(&pool).await;
    assert_eq!(records.len(), 2, "G1's success + the failed run");
    assert_eq!(records[1].0, "validation_failed");
    assert!(
        records[1].1.is_some(),
        "the failed run carries its end timestamp"
    );

    // The previous version stays active: G1 is still the newest published
    // reference and still adopted; a restarted API serves it.
    assert_eq!(newest_published(&pool).await, Some(g1));
    let state_after = api::state::AppState::boot(
        restarted_pool(&db_name).await,
        &repo_data_dir(),
        Default::default(),
    )
    .await
    .expect("API restart");
    assert_eq!(
        state_after.active.load_full().generation_id(),
        g1,
        "the previous version stays active through the validation failure"
    );

    drop_test_db(&db_name).await;
}

/// PERSISTENCE failure: an interrupted build leaves `status = building`
/// with incomplete projections; the previous version stays active and a
/// worker restart's retry restores the artifacts idempotently (same
/// generation id, no duplicates); the legacy dual-write tables were never
/// the only copy of anything.
#[tokio::test(flavor = "multi_thread")]
async fn persistence_failure_recovers_on_retry_without_a_new_generation() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    let g1 = first.published_generation_id.expect("G1 published");
    let _state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");

    // Build a candidate and inject the persistence failure: projections
    // partially lost, status back at `building` (an interrupted build).
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '9001'")
        .execute(&pool)
        .await
        .expect("content change");
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-s8")
        .await
        .expect("the candidate builds");
    let g2 = built.generation_id;
    assert_ne!(g1, g2, "the content change builds a new candidate");
    sqlx::query("DELETE FROM generation_trigram_surface WHERE generation_id = $1")
        .bind(g2)
        .execute(&pool)
        .await
        .expect("inject the persistence loss");
    sqlx::query(
        "UPDATE catalog_generations SET projection_status = 'building' WHERE generation_id = $1",
    )
    .bind(g2)
    .execute(&pool)
    .await
    .expect("mark the interrupted build");

    // The interrupted build is not a publication candidate.
    assert_eq!(newest_published(&pool).await, Some(g1));
    let state_after = api::state::AppState::boot(
        restarted_pool(&db_name).await,
        &repo_data_dir(),
        Default::default(),
    )
    .await
    .expect("API restart");
    assert_eq!(
        state_after.active.load_full().generation_id(),
        g1,
        "the previous version stays active through the persistence failure"
    );

    // Worker restart retry: the flow completes idempotently — the SAME
    // generation id, no duplicated artifacts, projections complete.
    let retry = restarted_pool(&db_name).await;
    let report = ingest::commands::publish::publish(&retry, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("retry publishes");
    assert_eq!(report.status, "success", "the retry completes: {report:?}");
    assert_eq!(
        report.published_generation_id,
        Some(g2),
        "identical content reuses the recorded generation id"
    );
    let surface: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM generation_trigram_surface WHERE generation_id = $1",
    )
    .bind(g2)
    .fetch_one(&pool)
    .await
    .expect("restored projections");
    assert!(surface > 0, "the lost artifacts are restored by the retry");

    let records = run_records(&pool).await;
    let last = records.last().expect("the retry's run record");
    assert_eq!(
        last.0, "success",
        "the run record reflects the completed retry"
    );

    drop_test_db(&db_name).await;
}

/// PROMOTION failure: the worker crashes between `validated` and the
/// promotion; the previous version stays active (an API restart still
/// serves G1) and the retry completes the promotion idempotently — the
/// reference advances only from a validated candidate with complete
/// projections.
#[tokio::test(flavor = "multi_thread")]
async fn promotion_failure_keeps_the_previous_active_until_the_retry_completes() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let first = publish(&pool).await;
    let g1 = first.published_generation_id.expect("G1 published");
    let _state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");

    // Build + validate only (the crash before the promotion).
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '9001'")
        .execute(&pool)
        .await
        .expect("content change");
    let taxonomy_version =
        ingest::support::compute_taxonomy_version(&repo_data_dir()).expect("taxonomy version");
    let built = db::generations::build::build_generation(&pool, &taxonomy_version)
        .await
        .expect("candidate builds");
    let g2 = built.generation_id;
    let taxonomy = taxonomy::loader::load_data_dir(&repo_data_dir()).expect("taxonomy loads");
    let gate = db::generations::validate::validate_generation(&pool, g2, Some(&taxonomy))
        .await
        .expect("validation runs");
    assert!(
        gate.passed(),
        "the candidate validates: {:?}",
        gate.failures
    );

    // API restart before the promotion: the previous version stays active.
    let state_after = api::state::AppState::boot(
        restarted_pool(&db_name).await,
        &repo_data_dir(),
        Default::default(),
    )
    .await
    .expect("API restart");
    assert_eq!(
        state_after.active.load_full().generation_id(),
        g1,
        "the promotion failure (crash before promote) leaves G1 active"
    );
    let (g2_status,): (String,) =
        sqlx::query_as("SELECT status::text FROM catalog_generations WHERE generation_id = $1")
            .bind(g2)
            .fetch_one(&pool)
            .await
            .expect("candidate manifest");
    assert_eq!(
        g2_status, "validated",
        "the un-promoted candidate stays validated, never published"
    );

    // The retry (worker restart) completes the promotion idempotently.
    let retry = restarted_pool(&db_name).await;
    let report = ingest::commands::publish::publish(&retry, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("retry publishes");
    assert_eq!(report.status, "success");
    assert_eq!(report.published_generation_id, Some(g2));
    let records = run_records(&pool).await;
    assert_eq!(records.last().expect("retry record").0, "success");

    drop_test_db(&db_name).await;
}
