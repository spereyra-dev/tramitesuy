//! S8 task 26 (catalog-generations delta "Previous generation remains
//! recoverable", spec §7 tests 3/6, R5/R13): the rollback path BEFORE cache
//! activation. Reactivating the retained previous generation restores its
//! taxonomy AND its search providers — searches after the rollback run
//! against the reactivated generation's taxonomy and per-generation
//! providers. A defective new generation is never mutated to fix it: the
//! rollback is a re-promotion of the retained previous generation.

mod support;

use support::*;

/// Publishes G1 and boots the API serving it; returns the generation id and
/// the search outcome captured under G1 (the rollback must reproduce it
/// exactly).
async fn boot_g1_and_capture(
    pool: &sqlx::PgPool,
) -> (
    sqlx::types::uuid::Uuid,
    serde_json::Value,
    api::state::AppState,
) {
    seed_read_fixture(pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;
    let (status, search) = request(&app, "GET", "/api/v1/search?q=veh%C3%ADculo").await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "G1 serves searches: {search}"
    );
    let g1 = db::generations::adopt::newest_published(pool)
        .await
        .expect("newest published")
        .expect("a published generation exists")
        .generation_id;
    // A fresh state over the same durable generation (the test's adoption
    // driver; it serves G1 until the tick swaps in a newer publication).
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot G1");
    (g1, search, state)
}

/// The projection row count of one generation (deletion/mutation audits).
async fn projection_rows_of(pool: &sqlx::PgPool, id: sqlx::types::uuid::Uuid) -> i64 {
    let mut total = 0;
    for table in [
        "generation_life_events",
        "generation_fts_text",
        "generation_trigram_surface",
        "generation_event_cards",
        "generation_procedure_details",
    ] {
        // Audited: table names are a fixed allowlist, never input.
        let audited = sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE generation_id = '{id}'"
        ));
        let rows: i64 = sqlx::query_scalar(audited)
            .fetch_one(pool)
            .await
            .expect("count rows");
        total += rows;
    }
    total
}

#[tokio::test(flavor = "multi_thread")]
async fn a_defective_generation_is_rolled_back_by_repromoting_the_previous_one() {
    let (pool, db_name) = fresh_migrated_db().await;
    let (g1, _g1_search, state) = boot_g1_and_capture(&pool).await;

    // G2 is published over changed content — and is defective (the fixture
    // renames a procedure; the ROLLBACK must never repair it).
    sqlx::query(
        "UPDATE procedures SET name = name || ' (v2 defectuoso)' WHERE external_id = '4551'",
    )
    .execute(&pool)
    .await
    .expect("content change");
    let g2 = publish_sample_generation(&pool).await.generation_id;
    assert_ne!(g1, g2, "the content change builds a new generation");
    // The API adopts G2 through the reconciliation tick.
    let tick = api::generation::reconcile::tick(&state)
        .await
        .expect("tick");
    assert_eq!(tick.adopted, Some(g2), "G2 is adopted");
    let app = api::build_router(state.clone());
    let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
    assert_eq!(
        body["name"], "Solicitud de empadronamientos (v2 defectuoso)",
        "the defective generation is live before the rollback: {status} {body}"
    );

    // The defective generation's projections must survive the rollback
    // untouched: snapshot their row count first.
    let g2_rows_before = projection_rows_of(&pool, g2).await;

    // Rollback: reactivate the retained previous generation (a re-promotion
    // of the retained G1, never a repair of G2).
    let reactivated = db::generations::adopt::reactivate(&pool, g1)
        .await
        .expect("reactivation runs");
    assert!(
        reactivated,
        "the retained previous generation is re-promoted"
    );

    // The reconciliation adopts the re-promoted G1 through the same
    // detection path as any publication.
    let rollback = api::generation::reconcile::tick(&state)
        .await
        .expect("tick");
    assert_eq!(
        rollback.adopted,
        Some(g1),
        "the reactivated previous generation is adopted: {rollback:?}"
    );

    // The rollback restored G1's data: the defective rename is gone from
    // the SERVED generation without touching G2's rows.
    let app = api::build_router(state.clone());
    let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["name"], "Solicitud de empadronamientos",
        "the served generation is G1 again after the rollback: {body}"
    );
    assert_eq!(
        projection_rows_of(&pool, g2).await,
        g2_rows_before,
        "the defective new generation's projections are untouched by the rollback"
    );

    common_drop(&db_name).await;
}

/// Searches after the rollback run against G1's taxonomy and its
/// per-generation providers: the outcome is identical to the G1-era search
/// (same ranking, same explanations).
#[tokio::test(flavor = "multi_thread")]
async fn searches_after_the_rollback_run_against_the_reactivated_generation() {
    let (pool, db_name) = fresh_migrated_db().await;
    let (g1, g1_search, state) = boot_g1_and_capture(&pool).await;

    // G2 is published and adopted; the API serves it.
    sqlx::query(
        "UPDATE procedures SET name = name || ' (v2 defectuoso)' WHERE external_id = '4551'",
    )
    .execute(&pool)
    .await
    .expect("content change");
    let g2 = publish_sample_generation(&pool).await.generation_id;
    assert_eq!(
        api::generation::reconcile::tick(&state)
            .await
            .expect("tick")
            .adopted,
        Some(g2)
    );

    // Rollback to the retained G1 and re-adopt.
    assert!(
        db::generations::adopt::reactivate(&pool, g1)
            .await
            .expect("reactivate")
    );
    let tick = api::generation::reconcile::tick(&state)
        .await
        .expect("tick");
    assert_eq!(tick.adopted, Some(g1), "G1 is re-adopted");

    // A search now runs against G1's taxonomy and providers again.
    let app = api::build_router(state.clone());
    let (status, search) = request(&app, "GET", "/api/v1/search?q=veh%C3%ADculo").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        search, g1_search,
        "the post-rollback search reproduces the G1-era outcome exactly"
    );

    common_drop(&db_name).await;
}
