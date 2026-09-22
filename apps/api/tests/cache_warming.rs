//! S10 task 32 (RED): cache warming from a static non-sensitive committed
//! list (OPT-05, search-cache delta "Cache warming from a static non-
//! sensitive list", R7). Warming runs after adoption through the normal
//! computation path WITHOUT fabricating user logs; publication is never
//! conditioned on warming; a failing warming leaves serving unaffected;
//! and warming an already-warm cache is a no-op.

mod support;

use std::sync::Arc;

use axum::http::StatusCode;

use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";
const EVENTS: &str = "/api/v1/events/comprar-vehiculo";

#[tokio::test(flavor = "multi_thread")]
async fn warming_caches_the_committed_list_without_user_logs() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    // Background warming is disabled so the test's explicit warming calls
    // are the only ones: the entry counts below stay deterministic.
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        limits_without_warming(),
    )
    .await;

    // Publication is already complete BEFORE warming runs: the adopted
    // generation is active and the snapshot serves catalog reads.
    assert!(
        state.active.load_full().is_loaded(),
        "publication completed before warming ran"
    );
    let (status, _) = request(&app, "GET", EVENTS).await;
    assert_eq!(status, StatusCode::OK, "the snapshot serves before warming");

    // Warming runs through the normal computation path.
    let warmed = api::cache::warming::warm(&state).await;

    // The committed list is non-empty and every listed query is cached.
    let committed = api::cache::warming::committed_queries();
    assert!(
        !committed.is_empty(),
        "the committed warming list is not empty"
    );
    assert_eq!(
        state.active.load_full().cache.entry_count(),
        committed.len(),
        "every listed query is cached after warming (distinct entries)"
    );
    assert_eq!(
        warmed,
        committed.len(),
        "warming reports the entries it warmed"
    );

    // Warming never fabricates user logs: no search_logs rows exist.
    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(&pool)
        .await
        .expect("search_logs readable");
    assert_eq!(
        logs, 0,
        "warming creates no user search_logs rows (no fabricated traffic)"
    );

    // The warmed queries now serve from the cache: a listed query is a
    // HIT (no computation), proving the warming went through the normal
    // insert path.
    let hits_before = metrics.cache_total(CacheEvent::Hit);
    let listed = &committed[0];
    let uri = format!("{SEARCH}?q={}", urlencode(listed));
    let (status, _) = request(&app, "GET", &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        metrics.cache_total(CacheEvent::Hit) - hits_before,
        1,
        "the warmed query serves from the cache (a hit, no computation)"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_warming_leaves_serving_unaffected() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    // Background warming is disabled so the test's explicit warming calls
    // are the only ones: the entry counts below stay deterministic.
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        limits_without_warming(),
    )
    .await;

    // Kill the database under the running state: every warming computation
    // fails, and warming must contain the failure — no panic, no cache
    // pollution, serving unaffected.
    common_drop(&db_name).await;

    let warmed = api::cache::warming::warm(&state).await;
    assert_eq!(
        warmed, 0,
        "a failing warming warms nothing and is reported, never propagated"
    );
    assert_eq!(
        state.active.load_full().cache.entry_count(),
        0,
        "a failing warming inserts nothing into the cache"
    );

    // Serving is unaffected: the snapshot reads keep serving (0 SQL).
    let (status, _) = request(&app, "GET", EVENTS).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a failing warming leaves snapshot serving untouched"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn warming_an_already_warm_cache_is_a_no_op() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    // Background warming is disabled so the test's explicit warming calls
    // are the only ones: the entry counts below stay deterministic.
    let (_app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        limits_without_warming(),
    )
    .await;

    // First warming computes and caches the committed list.
    let first = api::cache::warming::warm(&state).await;
    assert!(first > 0);
    let entries = state.active.load_full().cache.entry_count();
    let bytes = state.active.load_full().cache.bytes();

    // TRIANGULATE (task 32): warming an already-warm cache recomputes
    // nothing — every listed query is a cache hit through the normal path.
    let second = api::cache::warming::warm(&state).await;
    assert_eq!(
        second, 0,
        "the second warming computes nothing: already-warm entries serve"
    );
    assert_eq!(
        state.active.load_full().cache.entry_count(),
        entries,
        "an already-warm cache keeps its entries (no duplicates)"
    );
    assert_eq!(
        state.active.load_full().cache.bytes(),
        bytes,
        "an already-warm cache's byte accounting is untouched"
    );

    common_drop(&db_name).await;
}

/// Percent-encodes a committed query for a `q=` URI parameter (the
/// committed list is non-sensitive, fixed text).
fn urlencode(query: &str) -> String {
    let mut encoded = String::new();
    for byte in query.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Limits with background warming disabled (the tests call the warming
/// pass explicitly, so the counts stay deterministic).
fn limits_without_warming() -> api::config::ApiLimits {
    api::config::ApiLimits {
        cache_warming: false,
        ..api::config::ApiLimits::default()
    }
}
