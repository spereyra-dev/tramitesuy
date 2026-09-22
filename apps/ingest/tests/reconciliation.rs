//! S8 task 23 (catalog-generations delta, OPT-02/OPT-04, R10): publication
//! detection. The worker reconciles the durable manifest every configured
//! interval (60 s by default), reads the API's adoption write-back, and
//! raises the operational lag alert when the newest confirmed publication
//! is older than the bound without being adopted. The cross-process
//! notification (LISTEN/NOTIFY) is only an accelerator: a publication whose
//! notification is lost is adopted within one reconciliation cycle.
//! Reconciliation never deletes anything by itself.

mod common;

use common::*;

use ingest::commands::publish::Trigger;
use ingest::reconciliation::{self, ReconcileConfig};

/// The repository's `data/` directory (real YAML taxonomy for the build).
fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("data")
}

/// Seeds a small source catalog (taxonomy + two procedures + one relation).
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

    for (id, name) in [("9001", "Solicitud de alta de vehículos")] {
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

/// The adopted manifest row of one generation (id, adopted_at set?).
async fn adoption_of(pool: &sqlx::PgPool, generation: uuid::Uuid) -> (Option<uuid::Uuid>, bool) {
    let (active, adopted): (Option<uuid::Uuid>, Option<String>) = sqlx::query_as(
        "SELECT active_generation_id, adopted_at::text \
         FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(generation)
    .fetch_one(pool)
    .await
    .expect("manifest row");
    (active, adopted.is_some())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_notification_is_adopted_within_one_reconciliation_cycle() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    // The worker publishes G1; the API (fresh process) adopts at boot and
    // writes the adoption back on the manifest row.
    let first = ingest::commands::publish::publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish G1");
    let g1 = first.published_generation_id.expect("G1 published");
    let api_state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots G1");
    assert_eq!(
        api_state.active.load_full().generation_id(),
        g1,
        "the API serves G1 after boot"
    );

    // A new publication completes but its notification to the API is LOST:
    // the durable manifest is the only trace.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '9001'")
        .execute(&pool)
        .await
        .expect("content change");
    let second = ingest::commands::publish::publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish G2");
    let g2 = second.published_generation_id.expect("G2 published");
    assert_ne!(g1, g2, "the content change builds a new generation");

    // Without any signal the API keeps serving G1...
    assert_eq!(
        api_state.active.load_full().generation_id(),
        g1,
        "without detection the API keeps serving the current generation"
    );

    // One reconciliation cycle adopts the publication.
    let tick = api::generation::reconcile::tick(&api_state)
        .await
        .expect("tick runs");
    assert_eq!(
        tick.adopted,
        Some(g2),
        "the lost-notification publication is adopted"
    );

    // ...and the adoption is written back for the worker to confirm.
    let (active, adopted) = adoption_of(&pool, g2).await;
    assert_eq!(
        active,
        Some(g2),
        "the manifest records the adopted generation"
    );
    assert!(adopted, "the adoption carries its timestamp");

    // The worker's pass confirms the adoption (no lag, nothing to collect).
    let pass = reconciliation::run_pass(&pool, &ReconcileConfig::default())
        .await
        .expect("worker reconciliation pass");
    assert!(pass.adopted, "the worker confirms the API's adoption");
    assert!(
        !pass.lag_alert,
        "no lag alert once the publication is adopted"
    );

    drop_test_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_lagging_alert_fires_past_the_bound() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    // G1 published and confirmed adopted by the API.
    let first = ingest::commands::publish::publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish G1");
    let g1 = first.published_generation_id.expect("G1");
    let _api_state = api::state::AppState::boot(pool.clone(), &repo_data_dir(), Default::default())
        .await
        .expect("API boots");

    // A newer publication the API has NOT adopted, published well past the
    // (shortened, configurable) alert bound.
    sqlx::query("UPDATE procedures SET name = name || ' (v2)' WHERE external_id = '9001'")
        .execute(&pool)
        .await
        .expect("content change");
    let second = ingest::commands::publish::publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish G2");
    let g2 = second.published_generation_id.expect("G2");
    let lagging = ReconcileConfig {
        interval: std::time::Duration::from_secs(60),
        lag_alert_after: std::time::Duration::from_secs(0),
        ..ReconcileConfig::default()
    };
    let pass = reconciliation::run_pass(&pool, &lagging)
        .await
        .expect("worker pass");
    assert!(pass.lag_alert, "the lagging-API alert fires past the bound");
    assert!(!pass.adopted, "G2 is not adopted yet: {pass:?}");
    let _ = (g1, g2);

    // With the default bound the fresh publication is NOT yet lagging.
    let calm = reconciliation::run_pass(&pool, &ReconcileConfig::default())
        .await
        .expect("worker pass");
    assert!(!calm.lag_alert, "within the bound there is no lag alert");

    drop_test_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worker_pass_collects_only_after_confirmed_adoption() {
    let (pool, db_name) = fresh_migrated_db().await;
    // Four published manifest rows (G0..G3) with G3 adopted; retention 3.
    let mut ids = Vec::new();
    for age in ["4 hours", "3 hours", "2 hours", "1 hour"] {
        let id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO catalog_generations \
             (generation_id, status, content_hash, taxonomy_version, engine_version, \
              source_synced_at, event_count, procedure_count, projection_status, published_at) \
             VALUES ($1, 'published', 'hash', 'tax', 'engine', now(), 1, 1, 'complete', \
                     now() - $2::interval)",
        )
        .bind(id)
        .bind(age)
        .execute(&pool)
        .await
        .expect("published manifest");
        ids.push(id);
    }
    let g0 = ids[0];

    // Unconfirmed: an API lagging behind defers every collection.
    let report = reconciliation::run_pass(&pool, &ReconcileConfig::default())
        .await
        .expect("pass");
    assert!(
        report.deferred_unadopted,
        "no collection while adoption is unconfirmed"
    );
    assert!(report.collected.is_empty());

    // Adoption confirmed on the NEWEST publication: the beyond-retention
    // generation is collected (off the request path); the active and
    // previous ones stay.
    db::generations::adopt::confirm_adoption(&pool, ids[3], &[])
        .await
        .expect("adopt newest");
    let pass = reconciliation::run_pass(&pool, &ReconcileConfig::default())
        .await
        .expect("pass");
    assert!(
        !pass.deferred_unadopted,
        "collection is gated on the newest publication being confirmed adopted"
    );
    assert_eq!(
        pass.collected,
        vec![g0],
        "only the beyond-retention generation is collected"
    );
    // The collected manifest keeps its row (stamped retired), the active
    // and previous rows are untouched.
    let retired: Option<String> = sqlx::query_scalar(
        "SELECT retired_at::text FROM catalog_generations WHERE generation_id = $1",
    )
    .bind(g0)
    .fetch_one(&pool)
    .await
    .expect("collected manifest");
    assert!(retired.is_some(), "the manifest records the retirement");
    for kept in &ids[1..] {
        let retired: Option<String> = sqlx::query_scalar(
            "SELECT retired_at::text FROM catalog_generations WHERE generation_id = $1",
        )
        .bind(kept)
        .fetch_one(&pool)
        .await
        .expect("kept manifest");
        assert!(
            retired.is_none(),
            "the active/previous generations are never touched"
        );
    }
    // A second pass is idempotent: nothing further is collected.
    let again = reconciliation::run_pass(&pool, &ReconcileConfig::default())
        .await
        .expect("pass");
    assert!(again.collected.is_empty(), "collection is idempotent");

    drop_test_db(&db_name).await;
}

#[test]
fn reconciliation_configuration_defaults_and_overrides() {
    let defaults = ReconcileConfig::default();
    assert_eq!(
        defaults.interval,
        std::time::Duration::from_secs(60),
        "reconciliation runs every 60 seconds by default"
    );
    assert_eq!(
        defaults.lag_alert_after,
        std::time::Duration::from_secs(600),
        "the lag alert bound is 10 minutes by default"
    );
    assert_eq!(
        defaults.retention, 3,
        "three generations are retained by default"
    );

    let custom = ReconcileConfig::from_lookup(|name| match name {
        "INGEST_RECONCILE_SECS" => Some("15".to_string()),
        "INGEST_LAG_ALERT_SECS" => Some("120".to_string()),
        "INGEST_GENERATION_RETENTION" => Some("5".to_string()),
        "INGEST_RETENTION_WINDOW_SECS" => Some("600".to_string()),
        _ => None,
    })
    .expect("valid overrides");
    assert_eq!(custom.interval, std::time::Duration::from_secs(15));
    assert_eq!(custom.lag_alert_after, std::time::Duration::from_secs(120));
    assert_eq!(custom.retention, 5);
    assert_eq!(
        custom.retention_window,
        std::time::Duration::from_secs(600),
        "the in-flight retention window is configurable"
    );

    assert!(
        ReconcileConfig::from_lookup(|name| match name {
            "INGEST_RECONCILE_SECS" => Some("never".to_string()),
            _ => None,
        })
        .is_err(),
        "an unparseable interval is rejected at boot"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worker_publish_hints_the_notification_channel() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_source_catalog(&pool).await;

    // A listener that will receive the promotion hint (the accelerator; the
    // reconciliation interval remains the correctness mechanism).
    let mut listener = sqlx::postgres::PgListener::connect(&database_url_for(&db_name))
        .await
        .expect("listener connection");
    listener
        .listen(reconciliation::PUBLICATION_CHANNEL)
        .await
        .expect("listen on the publication channel");

    ingest::commands::publish::publish(&pool, &repo_data_dir(), Trigger::Manual)
        .await
        .expect("publish succeeds");

    let hint = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
        .await
        .expect("the publication hint arrives within the test timeout")
        .expect("the hint is readable");
    assert_eq!(
        hint.channel(),
        reconciliation::PUBLICATION_CHANNEL,
        "the worker publishes on the shared notification channel"
    );

    drop_test_db(&db_name).await;
}
