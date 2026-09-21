//! S7 task 22 (OPT-03, api delta cold-start, OPT-11): cold-start semantics.
//! The internal `/ready` route lives OUTSIDE the closed `/api/v1`
//! inventory and reports the active generation, its age, and the last
//! successful sync. Catalog reads return 503 until the first valid
//! snapshot load, and an invalid or failed load never changes the served
//! generation.

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn freshly_started_api_without_a_generation_is_not_ready_and_catalog_reads_503() {
    let (pool, _db) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    let (status, _) = request(&app, "GET", "/api/v1/categories").await;
    assert_eq!(
        status,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "catalog reads refuse traffic before the first valid snapshot load"
    );

    let (status, body) = request(&app, "GET", "/ready").await;
    assert_eq!(
        status,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "readiness must not declare ready without a valid snapshot: {body}"
    );
    assert_eq!(body["status"], "starting");
    assert!(body["generation"].is_null(), "no active generation: {body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn after_the_first_valid_load_the_api_is_ready_and_catalog_reads_come_from_the_snapshot() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    let (status, body) = request(&app, "GET", "/api/v1/categories").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_hyphen_slugs(&body);

    let (status, ready) = request(&app, "GET", "/ready").await;
    assert_eq!(status, axum::http::StatusCode::OK, "ready: {ready}");
    assert_eq!(ready["status"], "ready");
    let generation = &ready["generation"];
    assert!(
        !generation["generation_id"]
            .as_str()
            .expect("generation id")
            .is_empty(),
        "the readiness reports the active generation id: {ready}"
    );
    assert!(
        generation["age_seconds"].is_number(),
        "the readiness reports the generation age: {ready}"
    );
    assert_eq!(
        ready["last_successful_sync"]
            .as_str()
            .map(|s| !s.is_empty()),
        Some(true),
        "the readiness reports the last successful sync: {ready}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_load_never_changes_the_served_generation() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let published = publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot loads G1");
    assert_eq!(
        state.generation_id(),
        published.generation_id,
        "G1 is served"
    );

    // A publication candidate that the API cannot validate (its manifest
    // carries a taxonomy version the boot YAML does not hash to) is
    // rejected: the loader errors, `install` is never called, and the
    // served generation stays exactly as it was.
    sqlx::query(
        "UPDATE catalog_generations \
         SET taxonomy_version = 'mismatched-worker-version', published_at = now() - interval '1 second' \
         WHERE generation_id = $1",
    )
    .bind(published.generation_id)
    .execute(&pool)
    .await
    .expect("tamper the manifest version");
    // A second, newer candidate exists but is rejected; the fallback must
    // NOT silently adopt it either.
    let rejected_load = api::generation::load_published(&pool, &repo_root().join("data")).await;
    assert!(
        rejected_load.is_err(),
        "a manifest whose taxonomy_version does not match the boot YAML is rejected"
    );
    assert_eq!(
        state.generation_id(),
        published.generation_id,
        "the failed load left the previously served generation active"
    );
}

/// TRIANGULATE: the probe and its future metrics endpoint stay OUTSIDE the
/// closed `/api/v1` inventory (api delta: the seven routes are the whole
/// public surface; no probe route is registered under it).
#[tokio::test(flavor = "multi_thread")]
async fn the_readiness_route_sits_outside_the_closed_api_v1_inventory() {
    let (pool, _db) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    // /ready exists and is internal-only (never under /api/v1).
    let (status, _) = request(&app, "GET", "/ready").await;
    assert_ne!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "/ready must be registered outside the /api/v1 inventory"
    );
    let (status, _) = request(&app, "GET", "/api/v1/ready").await;
    assert_eq!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "no probe route may be added under the closed /api/v1 inventory"
    );
}
