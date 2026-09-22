//! S10 task 31 (RED): generation isolation for late requests (OPT-05,
//! search-cache delta "Generation isolation for late requests", R3). A
//! request captured under G1 that finishes after G2 is adopted answers
//! coherently with G1 and writes nothing into G2's cache: the late insert
//! lands only in G1's cache, G2's cache stays empty until its own
//! computations, and the late insert cannot evict a G2 entry.

mod support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use sqlx::postgres::PgPoolOptions;

use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";
const G1_QUERY: &str = "compre%20un%20auto%20usado";
const G2_QUERY: &str = "xyzabc%20sin%20coincidencia";

/// A one-connection pool builder: the API pool is pinned by the test while
/// setup and the G2 adoption use the separate setup pool, so the pinned
/// G1 request can only finish after the release below.
fn one_connection_pool(_url: &str) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
}

async fn wait_for_misses(metrics: &MemoryMetrics, count: u64) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while metrics.cache_total(CacheEvent::Miss) < count {
        assert!(
            tokio::time::Instant::now() < deadline,
            "requests never reached the cache-miss point"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_g1_request_writes_nothing_into_g2s_cache() {
    let (setup_pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&setup_pool).await;
    publish_sample_generation(&setup_pool).await;

    // The API pool holds ONE connection: acquiring it here pins a G1
    // request mid-flight (after its cache miss, before its providers) so
    // it can only finish AFTER the G2 adoption below.
    let api_url = format!(
        "{}/{}",
        std::env::var("TRAMITESUY_TEST_DB_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
            .trim_end_matches("/postgres"),
        db_name
    );
    let api_pool = one_connection_pool(&api_url)
        .connect(&api_url)
        .await
        .expect("one-connection API pool");
    let metrics = Arc::new(MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        api_pool,
        &repo_root().join("data"),
        limits_without_warming(),
        metrics.clone(),
    )
    .await
    .expect("boot the G1 state");
    let g1 = state.active.load_full();

    let handle = {
        let app = api::build_router(state.clone());
        tokio::spawn(async move { request(&app, "GET", &format!("{SEARCH}?q={G1_QUERY}")).await })
    };
    wait_for_misses(&metrics, 1).await;

    // Adopt G2 while the G1 request is still in flight: the swap installs
    // a brand-new, EMPTY cache for G2.
    adopt_changed_generation(&state, &setup_pool).await;
    let g2 = state.active.load_full();
    assert_eq!(
        g2.cache.entry_count(),
        0,
        "the G2 swap installs an empty cache"
    );

    // The late G1 request completes now (the pool is free again).
    let (status, body) = handle.await.expect("request task completes");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["results"][0]["procedures"][0]["name"], "Solicitud de empadronamientos",
        "the late G1 request answers coherently with G1 data (the OLD \
         procedure name), never G2's changed content"
    );

    // The late insert landed ONLY in G1's cache (or is discarded with it);
    // G2's cache stays empty until its own computations.
    assert_eq!(
        g1.cache.entry_count(),
        1,
        "the late G1 insert lands in G1's own cache"
    );
    assert_eq!(
        g2.cache.entry_count(),
        0,
        "G2's cache stays empty: the late G1 request wrote nothing into it"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_late_g1_insert_cannot_evict_a_g2_entry() {
    let (setup_pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&setup_pool).await;
    publish_sample_generation(&setup_pool).await;

    let api_url = format!(
        "{}/{}",
        std::env::var("TRAMITESUY_TEST_DB_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
            .trim_end_matches("/postgres"),
        db_name
    );
    let api_pool = one_connection_pool(&api_url)
        .connect(&api_url)
        .await
        .expect("one-connection API pool");
    let metrics = Arc::new(MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        api_pool,
        &repo_root().join("data"),
        limits_without_warming(),
        metrics.clone(),
    )
    .await
    .expect("boot the G1 state");

    // TRIANGULATE (task 31): pin a late G1 request, adopt G2, warm G2's
    // cache with its own computation, then let the late G1 insert land.
    let g1 = state.active.load_full();
    let handle = {
        let app = api::build_router(state.clone());
        tokio::spawn(async move { request(&app, "GET", &format!("{SEARCH}?q={G1_QUERY}")).await })
    };
    wait_for_misses(&metrics, 1).await;
    adopt_changed_generation(&state, &setup_pool).await;

    // Warm G2 with its own query: one entry in G2's cache.
    let (warm_status, _) = request(
        &api::build_router(state.clone()),
        "GET",
        &format!("{SEARCH}?q={G2_QUERY}"),
    )
    .await;
    assert_eq!(warm_status, StatusCode::OK);
    let g2 = state.active.load_full();
    assert_eq!(g2.cache.entry_count(), 1, "G2 holds its own warmed entry");
    let g2_bytes_before = g2.cache.bytes();

    // The late G1 request completes and commits into G1's cache.
    let (status, _) = handle.await.expect("request task completes");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        g1.cache.entry_count(),
        1,
        "the late G1 insert landed in G1's cache"
    );

    // G2's entry survived untouched — the late insert cannot evict it (a
    // different SearchCache object entirely), and G2's own query still
    // hits.
    assert_eq!(
        g2.cache.entry_count(),
        1,
        "the late G1 insert cannot evict the G2 entry"
    );
    assert_eq!(
        g2.cache.bytes(),
        g2_bytes_before,
        "G2's byte accounting is untouched by the late G1 insert"
    );
    section_hit_g2(&state).await;

    common_drop(&db_name).await;
}

/// Asserts the G2 warm entry still serves as a HIT through the router.
async fn section_hit_g2(state: &api::state::AppState) {
    let app = api::build_router(state.clone());
    let (status, _) = request(&app, "GET", &format!("{SEARCH}?q={G2_QUERY}")).await;
    assert_eq!(status, StatusCode::OK);
}
