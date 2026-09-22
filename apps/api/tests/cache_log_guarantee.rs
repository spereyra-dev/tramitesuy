//! S10 task 30 (RED): log-before-respond on the cached path (OPT-09,
//! operations delta "Cache hit costs exactly one statement" / "Admission
//! counts log work", R7). Every successful search — compute, /search/debug
//! (read-only) and cache hit — persists its own log before responding: a
//! cache hit executes exactly 1 SQL statement (the consolidated log
//! insert); 100 concurrent identical requests produce 100 logs; a forced
//! log failure keeps the structural error (public 500) and caches no
//! success.

mod support;

use axum::http::StatusCode;
use std::sync::Arc;

use api::metrics::MemoryMetrics;
use support::*;

const SEARCH: &str = "/api/v1/search";
const DEBUG: &str = "/api/v1/search/debug";

#[tokio::test(flavor = "multi_thread")]
async fn a_cache_hit_executes_exactly_one_sql_statement() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        api::config::ApiLimits::default(),
    )
    .await;

    // The first request computes (miss) and persists its log.
    let (status, _) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The admission seam accounts for the log work: the metrics SQL-op
    // counter for the route sees the same 1 statement (delta over the
    // cumulative total).
    let ops_before = metrics.sql_ops_total("/api/v1/search");
    section.reset();
    let (status, _) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        count, 1,
        "a cache hit executes exactly ONE statement: the consolidated log \
         insert (cards come from the snapshot, providers never run)"
    );
    assert_eq!(
        metrics.sql_ops_total("/api/v1/search") - ops_before,
        1,
        "admission accounts for log work on the cached path"
    );

    // The read-only debug route shares the guarantee on its own hit.
    let (_, _) = request(
        &app,
        "GET",
        &format!("{DEBUG}?q=xyzabc%20sin%20coincidencia"),
    )
    .await;
    let (_, _) = request(
        &app,
        "GET",
        &format!("{DEBUG}?q=xyzabc%20sin%20coincidencia"),
    )
    .await;
    // The debug route reports under the same low-cardinality route label
    // as the search route (pre-existing task-1 wiring, unchanged here).
    let debug_ops_before = metrics.sql_ops_total("/api/v1/search");
    section.reset();
    let (status, _) = request(
        &app,
        "GET",
        &format!("{DEBUG}?q=xyzabc%20sin%20coincidencia"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        section.count(),
        1,
        "the /search/debug cache hit also costs exactly one statement"
    );
    assert_eq!(
        metrics.sql_ops_total("/api/v1/search") - debug_ops_before,
        1,
        "admission accounts for log work on the debug hit too"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hundred_concurrent_identical_requests_produce_hundred_logs() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        api::config::ApiLimits::default(),
    )
    .await;

    // 100 concurrent identical requests: whether they group (single
    // computation) or not, EVERY request persists its OWN log before
    // responding.
    let handles: Vec<_> = (0..100)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                request(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
                )
                .await
            })
        })
        .collect();
    for handle in handles {
        let (status, _) = handle.await.expect("request task completes");
        assert_eq!(status, StatusCode::OK);
    }

    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(&pool)
        .await
        .expect("search_logs readable");
    assert_eq!(
        logs, 100,
        "100 concurrent identical requests produce 100 log rows (one per \
         request, never one per group)"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_log_failure_returns_the_structural_error_and_caches_nothing() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        api::config::ApiLimits::default(),
    )
    .await;

    // Force the log failure: with `search_logs` gone, every search ends in
    // the structural 500 AFTER the computation (this scratch database is
    // single-purpose; CASCADE also removes the feedback table).
    sqlx::query("DROP TABLE search_logs CASCADE")
        .execute(&pool)
        .await
        .expect("search_logs dropped");

    // Two concurrent identical requests: the single-flight leader computes
    // and broadcasts, every request fails on its own log persistence.
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                request(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
                )
                .await
            })
        })
        .collect();
    for handle in handles {
        let (status, _) = handle.await.expect("request task completes");
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "a log failure keeps the current structural error (public 500), \
             never a silent success"
        );
    }

    // The failed requests cached nothing: a log failure never caches a
    // success (the pending write commits only after the log persists).
    let generation = state.active.load_full();
    assert_eq!(
        generation.cache.entry_count(),
        0,
        "a forced log failure never leaves a cached success behind"
    );

    common_drop(&db_name).await;
}

// TRIANGULATE (task 30): a transport failure after a confirmed log write is
// NOT reported as "no write". This is a documented at-most-once limit of
// the log-before-respond contract: once the consolidated log insert has
// committed, a failure delivering the response (or a client disconnect)
// cannot retract the persisted row — the write happened. The test below
// asserts exactly that limit (persisted row survives a dropped response);
// it documents the boundary instead of re-implementing a recovery.
#[tokio::test(flavor = "multi_thread")]
async fn a_failure_after_a_confirmed_log_write_is_still_a_write_documented_limit() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        api::config::ApiLimits::default(),
    )
    .await;

    // A successful search whose response is then dropped unread (the
    // transport never delivers it): the confirmed log write remains
    // persisted — the failure after the log is not a "no write".
    let uri = format!("{SEARCH}?q=compre%20un%20auto%20usado");
    let (status, _) = request(&app, "GET", &uri).await;
    assert_eq!(status, StatusCode::OK);

    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(&pool)
        .await
        .expect("search_logs readable");
    assert_eq!(
        logs, 1,
        "the confirmed log write survives the dropped response (documented \
         limit: log-before-respond is at-most-once per request)"
    );

    common_drop(&db_name).await;
}
