//! Task 18 (S6): the promotion flow and run records
//! (`ingest::commands::publish`). The mandatory flow (ingestion delta,
//! OPT-02, R13, design §6.3): build → validate → persist complete artifacts →
//! mark `validated` → promote the reference; retryable and idempotent; a
//! failure after working-table updates must never leave those tables as the
//! only copy of the live version (the legacy dual-write stays in place and
//! the manifest governs what is live).

mod common;

use common::*;

use ingest::commands::publish::{publish, Trigger};
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

/// Seeds a small source catalog in the legacy (working) tables: taxonomy
/// events + a few ingested procedures + one relation, so the build is
/// non-empty and promotable.
async fn seed_source_catalog(pool: &sqlx::PgPool) {
    let data_dir = repo_data_dir();
    let taxonomy = taxonomy::loader::load_data_dir(&data_dir).expect("YAML taxonomy loads");
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
         WHERE e.slug = 'alta-vehiculo' AND p.external_id = '9001'",
    )
    .execute(pool)
    .await
    .expect("seed relation");
}

async fn legacy_table_counts(pool: &sqlx::PgPool) -> (i64, i64, i64, i64) {
    let procedures: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM procedures")
        .fetch_one(pool)
        .await
        .expect("count procedures");
    let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM procedure_versions")
        .fetch_one(pool)
        .await
        .expect("count versions");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM life_events")
        .fetch_one(pool)
        .await
        .expect("count events");
    let relations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM life_event_procedures")
        .fetch_one(pool)
        .await
        .expect("count relations");
    (procedures, versions, events, relations)
}

async fn run_rows(pool: &sqlx::PgPool) -> Vec<(String, Option<String>, serde_json::Value)> {
    sqlx::query(
        "SELECT status::text, finished_at::text, counts \
         FROM ingestion_runs ORDER BY started_at",
    )
    .map(|row: sqlx::postgres::PgRow| {
        (
            row.get::<String, _>(0),
            row.get::<Option<String>, _>(1),
            row.get::<serde_json::Value, _>(2),
        )
    })
    .fetch_all(pool)
    .await
    .expect("run rows")
}

/// The full flow: build → validate → mark validated → promote. The manifest
/// is published, the run record captures start/end/status/counts and both
/// generation references, and the legacy (working) tables are untouched by
/// the promotion — never the sole copy of the live version.
#[tokio::test(flavor = "multi_thread")]
async fn full_publish_promotes_and_records_the_run() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;
    let legacy_before = legacy_table_counts(&pool).await;

    let report = publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish succeeds");

    assert_eq!(report.status, "success");
    let Some(published) = report.published_generation_id else {
        panic!("a successful publish promotes a generation: {report:?}");
    };
    assert_eq!(
        report.candidate_generation_id,
        report.published_generation_id,
        "the built candidate is the published generation"
    );

    let (status, published_at): (String, Option<String>) = sqlx::query_as(
        "SELECT status::text, published_at::text FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(published)
    .fetch_one(&pool)
    .await
    .expect("manifest row exists");
    assert_eq!(status, "published", "the reference is promoted");
    assert!(
        published_at.is_some(),
        "a published generation carries its promotion timestamp"
    );

    let runs = run_rows(&pool).await;
    assert_eq!(runs.len(), 1, "one run record per publish");
    let (run_status, finished_at, counts) = &runs[0];
    assert_eq!(run_status, "success");
    assert!(
        finished_at.is_some(),
        "the run record captures its end timestamp"
    );
    assert!(
        counts.get("events").and_then(|v| v.as_i64()).unwrap_or(0) > 0,
        "the run record captures the built event count, got: {counts}"
    );
    let run_row = sqlx::query(
        "SELECT candidate_generation_id, published_generation_id, trigger::text, attempt \
         FROM ingestion_runs",
    )
    .fetch_one(&pool)
    .await
    .expect("run record exists");
    assert_eq!(
        run_row.get::<Option<Uuid>, _>("candidate_generation_id"),
        Some(published),
        "the run record references the candidate generation"
    );
    assert_eq!(
        run_row.get::<Option<Uuid>, _>("published_generation_id"),
        Some(published),
        "the run record references the published generation"
    );
    assert_eq!(run_row.get::<String, _>("trigger"), "manual");
    assert_eq!(run_row.get::<i16, _>("attempt"), 1);

    let legacy_after = legacy_table_counts(&pool).await;
    assert_eq!(
        legacy_after, legacy_before,
        "promotion must never touch the legacy working tables (dual-write stays)"
    );

    drop_test_db(&db_name).await;
}

/// Retry/restart semantics: a build that persisted complete artifacts but
/// crashed before promotion (simulated here with build + validate only, so
/// the manifest sits at `validated`) completes promotion idempotently on the
/// retry, without duplicating any artifact.
#[tokio::test(flavor = "multi_thread")]
async fn restart_between_build_and_promotion_completes_promotion_idempotently() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    // "Crash" before promotion: build and validate only.
    let built = db::generations::build::build_generation(&pool, "taxonomy-fixture-s6")
        .await
        .expect("build succeeds");
    let gate = db::generations::validate::validate_generation(&pool, built.generation_id, None)
        .await
        .expect("validation succeeds");
    assert!(gate.passed(), "the seeded catalog validates: {gate:?}");
    let (life_events_before, cards_before): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM generation_life_events), \
                (SELECT COUNT(*) FROM generation_event_cards)",
    )
    .fetch_one(&pool)
    .await
    .expect("projection counts");

    // The retry (a restarted worker running the same publish).
    let report = publish(&pool, &repo_data_dir(), Trigger::Recovery)
        .await
        .expect("retry succeeds");
    assert_eq!(report.status, "success");
    assert_eq!(
        report.published_generation_id,
        Some(built.generation_id),
        "the retry promotes the SAME generation — no new content version"
    );

    let (life_events_after, cards_after): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM generation_life_events), \
                (SELECT COUNT(*) FROM generation_event_cards)",
    )
    .fetch_one(&pool)
    .await
    .expect("projection counts");
    assert_eq!(life_events_after, life_events_before, "no duplicated artifacts");
    assert_eq!(cards_after, cards_before, "no duplicated artifacts");

    let generation_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM catalog_generations")
            .fetch_one(&pool)
            .await
            .expect("generation rows");
    assert_eq!(generation_rows, 1, "exactly one generation exists");

    drop_test_db(&db_name).await;
}

/// TRIANGULATE: re-running the publish over identical already-published
/// content produces no second generation.
#[tokio::test(flavor = "multi_thread")]
async fn republishing_identical_content_produces_no_second_generation() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    let first = publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("first publish succeeds");
    assert_eq!(first.status, "success");

    let second = publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("second publish succeeds");
    assert_eq!(second.status, "success");
    assert!(
        second.already_published,
        "identical content must not create a new content version"
    );
    assert_eq!(
        second.published_generation_id, first.published_generation_id,
        "the same generation stays published"
    );

    let generation_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM catalog_generations")
            .fetch_one(&pool)
            .await
            .expect("generation rows");
    assert_eq!(generation_rows, 1, "no second generation for identical content");

    drop_test_db(&db_name).await;
}

/// A failure after the working-table updates (here: a candidate that fails
/// the validation gate) never makes those tables the sole source of the live
/// version: the legacy tables keep their data, no generation is published,
/// and the failure is recorded on the run record.
#[tokio::test(flavor = "multi_thread")]
async fn validation_failure_never_makes_working_tables_the_sole_copy() {
    let (pool, db_name) = fresh_migrated_db().await;
    // Empty source: taxonomy only, zero procedures — the build succeeds, the
    // gate must reject it.
    let data_dir = repo_data_dir();
    let taxonomy = taxonomy::loader::load_data_dir(&data_dir).expect("YAML taxonomy loads");
    db::repos::taxonomy_seed::seed_taxonomy(&pool, &taxonomy)
        .await
        .expect("taxonomy seeds");

    let report = publish(&pool, &data_dir, Trigger::Manual)
        .await
        .expect("the flow itself completes (failures are recorded, not panics)");
    assert_eq!(report.status, "validation_failed");
    assert!(
        report.published_generation_id.is_none(),
        "a rejected candidate is never published"
    );
    assert!(
        !report.validation_failures.is_empty(),
        "the recorded failures name the empty catalog: {report:?}"
    );

    let published: Option<String> = sqlx::query_scalar(
        "SELECT status::text FROM catalog_generations WHERE status = 'published'",
    )
    .fetch_optional(&pool)
    .await
    .expect("published rows");
    assert!(published.is_none(), "no generation became live");

    let runs = run_rows(&pool).await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, "validation_failed");
    assert!(runs[0].1.is_some(), "the failed run finished with a timestamp");

    // The working tables still hold their data — the ingestion pipeline's
    // dual-write guarantees they are never the sole copy of live data, and a
    // previously published generation would remain the manifest's reference.
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM life_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert!(events > 0, "the legacy working tables are intact");

    drop_test_db(&db_name).await;
}

/// The ingestion exclusion: a run that cannot acquire the exclusion records
/// a `skipped` status and builds nothing (the API keeps serving throughout).
#[tokio::test(flavor = "multi_thread")]
async fn a_run_blocked_by_the_exclusion_is_skipped_and_recorded() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    // Another run holds the ingestion exclusion on its own connection.
    let mut holder = pool.acquire().await.expect("holder connection");
    sqlx::query("SELECT pg_advisory_lock(hashtext('tramitesuy:ingestion'))")
        .execute(&mut *holder)
        .await
        .expect("exclusion held by another run");

    let report = publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish returns without queueing");
    assert_eq!(report.status, "skipped");
    assert!(report.candidate_generation_id.is_none());
    assert!(report.published_generation_id.is_none());

    let runs = run_rows(&pool).await;
    assert_eq!(runs.len(), 1, "the skip is recorded, not queued");
    assert_eq!(runs[0].0, "skipped");
    assert!(runs[0].1.is_some(), "the skipped run finishes immediately");

    sqlx::query("SELECT pg_advisory_unlock(hashtext('tramitesuy:ingestion'))")
        .execute(&mut *holder)
        .await
        .expect("exclusion released");
    drop(holder);

    // Once the exclusion is released, a retry promotes normally.
    let retry = publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("retry succeeds");
    assert_eq!(retry.status, "success");

    drop_test_db(&db_name).await;
}
