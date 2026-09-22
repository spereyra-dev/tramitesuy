//! S12 task 39 (RED): deadline and acquisition-timeout error contract
//! (OPT-10, api delta "Deadline exceeded returns 504", operations delta,
//! R11).
//!
//! All admitted work (compute, log, payload) runs inside
//! `tokio::time::timeout(search_deadline)`; elapsing answers 504 with the
//! documented structured error body — distinct from the overload 503 (no
//! `Retry-After`, no retry invitation) and carrying no internal detail
//! (no SQL text, stack, or timings). Pool exhaustion within
//! `acquire_timeout` answers the SAME 503 + `Retry-After` overload shape
//! instead of waiting the pool's 30 s default. A request cancelled by the
//! deadline releases its admission permit, single-flight holder, and
//! generation `Arc`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use api::config::ApiLimits;
use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";

/// Deadline limits under test: a fast 300 ms deadline (configuration-
/// driven; the production default is 2 s) with a distinctive Retry-After
/// value for the overload-shape assertions.
fn deadline_limits() -> ApiLimits {
    ApiLimits {
        search_deadline: Duration::from_millis(300),
        retry_after_seconds: 5,
        ..limits_without_warming()
    }
}

/// A computation exceeding the deadline returns 504 with the documented
/// body — distinct from the 503 overload response (no Retry-After).
#[tokio::test(flavor = "multi_thread")]
async fn computation_exceeding_the_deadline_returns_504() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        deadline_limits(),
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    let handle = {
        let app = app.clone();
        tokio::spawn(async move {
            request_with_headers(
                &app,
                "GET",
                &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
            )
            .await
        })
    };
    let miss_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while metrics.cache_total(CacheEvent::Miss) < 1 {
        assert!(
            tokio::time::Instant::now() < miss_deadline,
            "the request never reached the computation"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // The deadline window (300 ms) elapses while the computation is still
    // blocked: the API must answer 504 on its own, not hang.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        handle.is_finished(),
        "the deadline must terminate the admitted work within its window"
    );
    let (status, headers, body) = handle.await.expect("request task completes");
    assert_eq!(status, axum::http::StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["error"], "search deadline exceeded");
    assert!(
        headers.get("retry-after").is_none(),
        "a 504 must never carry Retry-After: proxies may retry a 503 but must not retry a 504"
    );
    // Distinct from the overload response (503 + Retry-After).
    assert_ne!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(body["error"], "overloaded");

    lock_tx.commit().await.expect("release the barrier");
    common_drop(&_name).await;
}

/// The 504 body is the documented structured shape and exposes no
/// internals: no SQL text, no pool diagnostics, no timings.
#[tokio::test(flavor = "multi_thread")]
async fn deadline_error_exposes_no_internals() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        deadline_limits(),
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    let handle = {
        let app = app.clone();
        tokio::spawn(async move {
            request_with_headers(
                &app,
                "GET",
                &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
            )
            .await
        })
    };
    let miss_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while metrics.cache_total(CacheEvent::Miss) < 1 {
        assert!(
            tokio::time::Instant::now() < miss_deadline,
            "the request never reached the computation"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(handle.is_finished(), "the deadline must answer");
    let (_status, _headers, body) = handle.await.expect("request task completes");

    assert_eq!(
        body,
        serde_json::json!({ "error": "search deadline exceeded" }),
        "the 504 body is exactly the documented shape with no internals"
    );

    lock_tx.commit().await.expect("release the barrier");
    common_drop(&_name).await;
}

/// Exhausting the pool answers the SAME 503 + `Retry-After` overload
/// shape within `acquire_timeout`, never the pool's 30 s default wait.
#[tokio::test(flavor = "multi_thread")]
async fn exhausted_pool_returns_503_rather_than_waiting_30s() {
    let (pool, db_name) = fresh_migrated_db().await;
    drop(pool);
    // A ONE-connection pool whose own acquire timeout is the sqlx 30 s
    // default: only the API's configured `acquire_timeout` (300 ms) can
    // bound the wait — proving the API never waits 30 s.
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    let url = format!("{}/{}", base.trim_end_matches("/postgres"), db_name);
    let one_conn = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("one-connection pool over the scratch database");

    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_limits_state_and_metrics(
        one_conn.clone(),
        metrics.clone(),
        ApiLimits {
            acquire_timeout: Duration::from_millis(300),
            search_deadline: Duration::from_secs(10),
            retry_after_seconds: 7,
            ..limits_without_warming()
        },
    );

    let guard = one_conn.acquire().await.expect("hold the only connection");
    let started = tokio::time::Instant::now();
    let handle = {
        let app = app.clone();
        tokio::spawn(
            async move { request_with_headers(&app, "GET", &format!("{SEARCH}?q=auto")).await },
        )
    };
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        handle.is_finished(),
        "an exhausted pool must answer within the configured acquire timeout"
    );
    let (status, headers, _body) = handle.await.expect("request task completes");
    assert_eq!(
        status,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "pool exhaustion inside acquire_timeout is the overload contract"
    );
    assert_eq!(
        headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("7")
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the API must never wait the pool's 30 s default"
    );

    drop(guard);
    let (status, _body) = request(&app, "GET", &format!("{SEARCH}?q=auto")).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    common_drop(&db_name).await;
}

/// TRIANGULATE: a request cancelled by the deadline releases its
/// admission permit, its single-flight holder, and its generation `Arc`.
#[tokio::test(flavor = "multi_thread")]
async fn deadline_cancellation_releases_permit_holder_and_generation() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        // ONE permit + a 300 ms deadline: the blocked computation is
        // cancelled by the deadline while holding the only permit.
        ApiLimits {
            search_deadline: Duration::from_millis(300),
            max_concurrent_searches: 1,
            ..deadline_limits()
        },
    )
    .await;

    let generation = state.active.load_full();
    let baseline = Arc::strong_count(&generation);

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    let handle = {
        let app = app.clone();
        tokio::spawn(async move {
            request_with_headers(
                &app,
                "GET",
                &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
            )
            .await
        })
    };
    let miss_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while metrics.cache_total(CacheEvent::Miss) < 1 {
        assert!(
            tokio::time::Instant::now() < miss_deadline,
            "the request never reached the computation"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // In-flight, the request holds its own captured generation Arc (the
    // one permit) and its single-flight leader holder.
    assert!(
        Arc::strong_count(&generation) > baseline,
        "the in-flight request retains the captured generation Arc"
    );
    assert_eq!(state.active.load_full().cache.inflight_count(), 1);

    let (status, _headers, _body) = handle.await.expect("request task completes");
    assert_eq!(status, axum::http::StatusCode::GATEWAY_TIMEOUT);

    // Everything the cancelled request held is released: the single
    // -flight holder is abandoned, the generation Arc count is back to
    // the baseline, and the admission permit is free again.
    assert_eq!(
        state.active.load_full().cache.inflight_count(),
        0,
        "the deadline-cancelled leader abandons its holder"
    );
    assert_eq!(
        Arc::strong_count(&generation),
        baseline,
        "the deadline-cancelled request released the generation Arc"
    );

    lock_tx.commit().await.expect("release the barrier");
    let (status, _body) = request(&app, "GET", &format!("{SEARCH}?q=auto")).await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "the permit was released: a fresh search is admitted again"
    );
    common_drop(&_name).await;
}
