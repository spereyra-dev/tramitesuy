//! S7 task 20 (OPT-02/OPT-04, R3): the active-generation holder. Every
//! request captures `state.active.load_full()` as its first operation and
//! keeps that `Arc<ActiveGeneration>` for payload, log, and providers; a
//! request that captured G1 and finishes after a swap to G2 answers
//! coherently with G1 data only (never mixes generations), and new requests
//! see G2 in full. The captured `Arc` strong count keeps the old generation
//! alive until the request drops it.

mod support;

use std::sync::Arc;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_late_request_answers_coherently_with_its_captured_generation() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;

    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot loads the published generation");
    let g1 = publish_sample_generation(&pool).await;
    assert_eq!(
        state
            .active
            .load_full()
            .manifest()
            .expect("loaded")
            .generation_id,
        g1.generation_id,
        "G1 is the active generation"
    );

    // The late request captures G1 as its first operation.
    let late = state.active.load_full();
    assert!(late.is_loaded(), "the captured request holds G1 in full");

    // A new publication builds G2 over changed content and is adopted.
    let g2 = adopt_changed_generation(&state, &pool).await;

    // The late request keeps answering with G1 data only: its captured Arc
    // still resolves the G1 manifest and G1 catalog content.
    assert_eq!(
        late.manifest().expect("G1 manifest").generation_id,
        g1.generation_id,
        "the captured Arc never mixes generations"
    );
    let cards = late.cards("comprar-vehiculo").expect("G1 cards");
    assert_eq!(cards.len(), 2, "G1 card set is untouched by the swap");
    let detail = late.procedure("4551").expect("G1 detail");
    assert_eq!(
        detail.name, "Solicitud de empadronamientos",
        "G1 data is exactly what G1 published"
    );

    // New requests see G2 in full.
    assert_eq!(
        state
            .active
            .load_full()
            .manifest()
            .expect("G2 manifest")
            .generation_id,
        g2.generation_id
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn new_http_requests_serve_the_swapped_generation_in_full() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot loads G1");
    let app = api::build_router(state.clone());

    // G2 publishes a renamed procedure; the swap installs it atomically.
    adopt_changed_generation(&state, &pool).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(
        body["name"], "Solicitud de empadronamientos (cambiado)",
        "a new request reads the snapshot of the adopted generation: {body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_captured_arc_keeps_the_old_generation_alive_until_the_request_drops_it() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let state = api::state::AppState::boot(
        pool.clone(),
        &repo_root().join("data"),
        // Warming off: a background warming pass would legitimately hold a
        // generation Arc of its own and inflate the strong-count contract
        // this test pins (task 20's captured-Arc boundary).
        limits_without_warming(),
    )
    .await
    .expect("boot loads G1");

    let late = state.active.load_full();
    let captured_count = Arc::strong_count(&late);

    adopt_changed_generation(&state, &pool).await;

    // After the swap only the request's Arc still holds the old generation.
    assert_eq!(
        Arc::strong_count(&late),
        captured_count - 1,
        "the holder released its reference; the request Arc is the only \
         remaining one, which is what keeps the old generation alive until \
         the request finishes"
    );

    // The request finishing (dropping its Arc) releases the generation; the
    // holder now serves the new generation only.
    drop(late);
    assert!(
        state.active.load_full().is_loaded(),
        "the holder serves the adopted generation after the request drains"
    );
}
