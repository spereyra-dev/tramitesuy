//! S12 task 37 (RED): query-length validation before any side effect
//! (OPT-10, api delta "Query length limit validated before any processing").
//!
//! A `q` over `q_max_chars` (512) Unicode characters or `q_max_bytes`
//! (2048) UTF-8 bytes is rejected with 400 BEFORE normalization, before any
//! cache lookup or insertion, and before any SQL statement is issued — no
//! search-log row, no candidate-provider query, untouched cache. Exactly
//! 512 characters within 2 KiB proceeds through the normal pipeline. Both
//! limits are configuration-driven, and multi-byte characters are counted
//! by Unicode scalar (510 'á' chars / 1020 bytes pass; 513 fail).

mod support;

use std::sync::Arc;

use api::config::ApiLimits;
use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";

/// The `search_logs` row count in the scratch database.
async fn log_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(pool)
        .await
        .expect("search_logs count readable")
}

#[tokio::test(flavor = "multi_thread")]
async fn over_length_q_is_rejected_before_any_side_effect() {
    let (pool, section) = fresh_counting_db_section().await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_limits_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        ApiLimits::default(),
    );

    section.reset();
    let (status, _headers, body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={}", "a".repeat(600))).await;

    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "bad request");
    // No SQL ran: no candidate-provider query, no log insert (the log row
    // count follows from zero statements, asserted explicitly anyway).
    assert_eq!(
        section.count(),
        0,
        "an over-length q must reach no SQL statement"
    );
    assert_eq!(log_count(&pool).await, 0, "no search-log row is created");
    // The cache was untouched: not even a miss was registered, and no
    // entry was inserted.
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 0);
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 0);
    assert_eq!(metrics.cache_total(CacheEvent::Compute), 0);
    assert_eq!(state.active.load_full().cache.entry_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn exactly_512_chars_within_2kib_is_processed_normally() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let app = spawn_app_with_metrics(pool.clone(), metrics.clone());

    let (status, _headers, body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={}", "a".repeat(512))).await;

    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(body["mode"].is_string(), "the normal pipeline answered");
    // It reached the cache path (a miss computed and served), proving the
    // request went through the normal pipeline rather than being rejected.
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 1);
    assert_eq!(metrics.cache_total(CacheEvent::Compute), 1);
    assert_eq!(log_count(&pool).await, 1);

    common_drop(&_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn both_limits_are_configuration_driven() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;

    // A smaller character limit rejects a query the default would accept.
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_limits_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        ApiLimits {
            q_max_chars: 10,
            ..ApiLimits::default()
        },
    );
    let (status, _headers, _body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={}", "a".repeat(12))).await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

    // A smaller byte limit rejects a query the character limit alone
    // would accept (10 ASCII chars, 10 bytes over an 8-byte limit).
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_limits_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        ApiLimits {
            q_max_bytes: 8,
            ..ApiLimits::default()
        },
    );
    let (status, _headers, _body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={}", "a".repeat(10))).await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

    common_drop(&_name).await;
}

/// TRIANGULATE: multi-byte characters are counted by Unicode scalar —
/// 510 'á' characters (1020 UTF-8 bytes) pass both limits; 513 fail the
/// character limit even though the byte count stays small.
#[tokio::test(flavor = "multi_thread")]
async fn multi_byte_chars_are_counted_by_unicode_scalar() {
    let (pool, _name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;

    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_limits_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        ApiLimits::default(),
    );
    let query: String = "á".repeat(510);
    let (status, _headers, _body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={query}")).await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let query: String = "á".repeat(513);
    let (status, _headers, _body) =
        request_with_headers(&app, "GET", &format!("{SEARCH}?q={query}")).await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

    common_drop(&_name).await;
}
