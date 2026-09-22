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

/// S14 task 45 (spec §7 test 2, OPT-02/OPT-04): concurrent-update
/// coherence DURING a swap. Search requests that overlap the G1→G2 swap
/// each answer INTERNALLY coherent with exactly one generation: the
/// procedure names in every response match G1 exactly or G2 exactly —
/// never a mixture (G2 renames procedure 4551 only, so a hypothetical
/// mix would pair the changed name with G1's unchanged companion). The
/// atomic ArcSwap capture makes that true by construction; this test
/// pins the invariant under real concurrency.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_during_a_swap_stay_coherent_with_one_generation() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let state =
        api::state::AppState::boot(pool.clone(), &repo_root().join("data"), Default::default())
            .await
            .expect("boot loads G1");
    let app = api::build_router(state.clone());

    // G1 payload: procedure 4551 carries its original name everywhere.
    let g1_name = "Solicitud de empadronamientos".to_string();
    // G2 renames exactly that procedure.
    let g2_name = format!("{g1_name} (cambiado)");
    let companion = "Alta de vehículos ante la DNT".to_string();

    // The swap runs CONCURRENTLY with the request storm: the adoption
    // task executes while 24 searches are in flight over both
    // generations.
    let swap = tokio::spawn({
        let state = state.clone();
        let pool = pool.clone();
        async move { adopt_changed_generation(&state, &pool).await }
    });

    let mut handles = Vec::new();
    for _ in 0..24 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await
        }));
    }
    let _g2 = swap.await.expect("the swap completes");

    for handle in handles {
        let (status, body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK, "{body}");
        assert_eq!(body["mode"], "open");
        let names: Vec<String> = body["results"][0]["procedures"]
            .as_array()
            .expect("cards served")
            .iter()
            .map(|card| card["name"].as_str().expect("card name").to_string())
            .collect();
        assert_eq!(
            names,
            vec![g1_name.clone(), companion.clone()],
            "the response is G1-coherent (a lone changed name would be a \
             mixed-generation payload): {body}"
        );
    }

    // After the swap, EVERY new request sees G2 in full — no response in
    // the storm mixed G1's name with G2's.
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let names: Vec<String> = body["results"][0]["procedures"]
        .as_array()
        .expect("cards served")
        .iter()
        .map(|card| card["name"].as_str().expect("card name").to_string())
        .collect();
    assert_eq!(
        names,
        vec![g2_name, companion],
        "new requests serve the adopted generation in full: {body}"
    );
}
