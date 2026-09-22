//! S12 task 38 (RED): admission control over total work (OPT-10, api delta
//! "Controlled overload response", operations delta "Admission controls
//! total work and rejects in a bounded way", R11).
//!
//! A `tokio::sync::Semaphore(max_concurrent_searches)` is held across the
//! WHOLE admitted work — compute, log persistence, and payload — shared by
//! `/api/v1/search` and `/api/v1/search/debug` (no separate debug budget).
//! Saturation answers 503 + `Retry-After` immediately, without enqueueing
//! the rejected request into any queue: in-flight work never exceeds the
//! limit even while the admitted requests are still persisting their logs,
//! and a cancelled request releases its permit (no unbounded work).

mod support;

use std::sync::Arc;
use std::time::Duration;

use api::config::ApiLimits;
use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";
const DEBUG: &str = "/api/v1/search/debug";

/// The admission limits under test: a small, deterministic budget (2
/// admitted searches) with a distinctive `Retry-After` value (3 s) and a
/// long deadline so the deadline contract (task 39) never interferes.
fn admission_limits() -> ApiLimits {
    ApiLimits {
        max_concurrent_searches: 2,
        retry_after_seconds: 3,
        search_deadline: Duration::from_secs(30),
        ..limits_without_warming()
    }
}

/// Barrier helper: polls until the given signal reaches exactly `count`,
/// with a bounded deadline (the tests block admitted work on a locked
/// table, so the metrics signals are the only progress observable).
async fn wait_for_signal(signals: impl Fn() -> u64, count: u64, what: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while signals() != count {
        assert!(
            tokio::time::Instant::now() < deadline,
            "requests never reached the {what} point"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn log_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(pool)
        .await
        .expect("search_logs count readable")
}

/// Saturation is rejected, not queued: while 2 admitted searches are
/// blocked mid-computation, 3 more arrivals each receive 503 +
/// `Retry-After: 3` immediately; exactly the admitted two ever compute;
/// after the barrier releases, only the admitted two complete (the
/// rejected three were never queued — they are already answered).
#[tokio::test(flavor = "multi_thread")]
async fn saturation_rejects_the_next_searches_without_queueing() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        admission_limits(),
    )
    .await;

    // Barrier: the admitted requests' candidate queries block on the
    // locked table, so both stay in flight (holding their permits) while
    // the rejections are observed.
    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    // Two DIFFERENT queries: each is its own single-flight leader, so both
    // admitted searches block on the locked table while holding their
    // permits (an identical second query would merely join the leader as
    // a waiter and never compute).
    let admitted: Vec<_> = (0..2)
        .map(|i| {
            let app = app.clone();
            tokio::spawn(async move {
                request(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=compre%20un%20auto%20usado%20numero%20{i}"),
                )
                .await
            })
        })
        .collect();
    wait_for_signal(
        || metrics.cache_total(CacheEvent::Miss),
        2,
        "cache-miss (admitted)",
    )
    .await;

    let rejected: Vec<_> = (0..3)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                request_with_headers(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=otra%20consulta%20distinta"),
                )
                .await
            })
        })
        .collect();
    for handle in rejected {
        let (status, headers, _body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            headers
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("3"),
            "the overload contract carries the configured Retry-After"
        );
    }

    // In-flight work never exceeded the limit: only the admitted two ever
    // started computing, and none of the work finished (the logs are
    // still pending — the barrier is still held).
    assert_eq!(metrics.cache_total(CacheEvent::Compute), 2);
    assert_eq!(log_count(&pool).await, 0);

    lock_tx.commit().await.expect("release the barrier");
    for handle in admitted {
        let (status, _body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    assert_eq!(log_count(&pool).await, 2);
    common_drop(&_name).await;
}

/// TRIANGULATE (operations delta "Admission counts log work"): with the
/// candidate queries done, two admitted requests block INSIDE their log
/// persistence (locked `search_logs`) and stay in flight; a third arrival
/// is rejected 503 + `Retry-After` while the logs are still being
/// persisted — the permit spans the log work, not only compute.
#[tokio::test(flavor = "multi_thread")]
async fn admission_counts_log_work() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        admission_limits(),
    )
    .await;

    // Barrier on the LOG table: compute completes, the consolidated log
    // insert blocks. Each admitted request reports its 3 provider
    // statements to the metrics seam BEFORE persisting, so
    // `sql_ops_total` == 6 proves both requests are past compute and
    // inside (or entering) the blocked log path.
    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE search_logs IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the log table");

    let admitted: Vec<_> = (0..2)
        .map(|i| {
            let app = app.clone();
            tokio::spawn(async move {
                request(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=compre%20un%20auto%20usado%20numero%20{i}"),
                )
                .await
            })
        })
        .collect();
    wait_for_signal(
        || metrics.sql_ops_total("/api/v1/search"),
        6,
        "post-compute (log phase)",
    )
    .await;

    let (status, headers, _body) = request_with_headers(
        &app,
        "GET",
        &format!("{SEARCH}?q=otra%20consulta%20distinta"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("3"),
        "the saturation while logs persist is the same overload contract"
    );
    // The logs are still being persisted: neither admitted request has
    // finished (both are blocked inside their log insert on the barrier).
    assert!(
        !admitted.iter().any(|handle| handle.is_finished()),
        "the admitted requests are still in flight (log phase)"
    );

    lock_tx.commit().await.expect("release the barrier");
    for handle in admitted {
        let (status, _body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    assert_eq!(log_count(&pool).await, 2);
    common_drop(&_name).await;
}

/// Cancellation leaves no unbounded work: aborting an admitted request
/// mid-computation releases its permit — a subsequent search is admitted
/// again instead of inheriting a leaked permit — and the single-flight
/// holder registered by the cancelled leader is abandoned.
#[tokio::test(flavor = "multi_thread")]
async fn cancellation_releases_the_permit_and_holder() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        // A ONE-permit budget makes the saturation/cancellation sequence
        // deterministic: the in-flight request holds the only permit.
        ApiLimits {
            max_concurrent_searches: 1,
            ..admission_limits()
        },
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    let admitted = {
        let app = app.clone();
        tokio::spawn(async move {
            request(
                &app,
                "GET",
                &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
            )
            .await
        })
    };
    wait_for_signal(
        || metrics.cache_total(CacheEvent::Miss),
        1,
        "admitted compute",
    )
    .await;

    // Saturated while the request is in flight...
    let (status, _headers, _body) = request_with_headers(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);

    // ...then cancelled: the permit and the single-flight holder are
    // released with the dropped future (polled with a deadline — abort
    // processes the cancellation asynchronously).
    admitted.abort();
    wait_for_signal(
        || u64::try_from(state.active.load_full().cache.inflight_count()).unwrap_or(1),
        0,
        "cancelled leader's holder released",
    )
    .await;
    assert_eq!(
        state.active.load_full().cache.inflight_count(),
        0,
        "the cancelled leader abandons its in-flight holder"
    );

    lock_tx.commit().await.expect("release the barrier");
    // A fresh search is admitted again: no permit leaked.
    let (status, _body) = request(&app, "GET", &format!("{SEARCH}?q=auto")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    common_drop(&_name).await;
}

/// TRIANGULATE: `/search/debug` (read-only) shares the SAME limiter —
/// with both permits held by `/search` requests, a debug request is
/// rejected 503 + `Retry-After` (no separate debug budget).
#[tokio::test(flavor = "multi_thread")]
async fn debug_shares_the_same_limiter() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        admission_limits(),
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    let admitted: Vec<_> = (0..2)
        .map(|i| {
            let app = app.clone();
            tokio::spawn(async move {
                request(
                    &app,
                    "GET",
                    &format!("{SEARCH}?q=compre%20un%20auto%20usado%20numero%20{i}"),
                )
                .await
            })
        })
        .collect();
    wait_for_signal(
        || metrics.cache_total(CacheEvent::Miss),
        2,
        "admitted compute",
    )
    .await;

    let (status, headers, _body) = request_with_headers(
        &app,
        "GET",
        &format!("{DEBUG}?q=compre%20un%20auto%20usado"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("3")
    );

    lock_tx.commit().await.expect("release the barrier");
    for handle in admitted {
        let (status, _body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK);
    }
    common_drop(&_name).await;
}
