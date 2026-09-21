//! Task 1 (OPT-10, R14, operations-delta observability): the privacy-safe
//! metrics seam. A served `/api/v1/search` request increments the
//! search-latency and SQL-op counters, and no emitted label ever carries
//! the submitted query text, normalized text, or a cache-key fingerprint.

mod support;

use std::sync::Arc;

use api::metrics::{CacheEvent, GenerationState, MemoryMetrics, Metrics};
use support::*;

const QUERY: &str = "compre un auto";
const SEARCH_ROUTE: &str = "/api/v1/search";
const EVENTS_ROUTE: &str = "/api/v1/events/{slug}";

#[tokio::test(flavor = "multi_thread")]
async fn search_request_increments_latency_and_sql_op_counters_privately() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let app = spawn_app_with_metrics(pool, metrics.clone());

    let (status, _) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto").await;
    assert_eq!(status, 200, "fixture search serves an open result");

    assert!(
        metrics.request_count(SEARCH_ROUTE, 200) >= 1,
        "search latency counter recorded the request"
    );
    assert!(
        metrics.sql_ops_total(SEARCH_ROUTE) >= 2,
        "SQL-op counter recorded at least the FTS + trigram statements"
    );
    for label in metrics.all_labels() {
        assert!(
            !label.contains(QUERY),
            "privacy: no metric label may carry the query text (found {label:?})"
        );
    }
}

/// TRIANGULATE: the same seam covers a catalog route (`/api/v1/events/{slug}`)
/// with the same privacy guarantee; the recorded budget is 0 (snapshot
/// serving, S7 task 20).
#[tokio::test(flavor = "multi_thread")]
async fn catalog_route_records_sql_ops_and_latency_privately() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let app = spawn_app_with_generation_and_metrics(pool, metrics.clone()).await;

    let (status, _) = request(&app, "GET", "/api/v1/events/comprar-vehiculo").await;
    assert_eq!(status, 200);

    assert_eq!(
        metrics.request_count(EVENTS_ROUTE, 200),
        1,
        "event-page latency counter recorded the request"
    );
    assert_eq!(
        metrics.sql_ops_total(EVENTS_ROUTE),
        0,
        "S7 task 20: the snapshot-served event page issues zero SQL \
         statements (previously 2 on the legacy by_event path)"
    );
    for label in metrics.all_labels() {
        assert!(
            !label.contains("comprar-vehiculo"),
            "privacy: no metric label may carry request slugs (found {label:?})"
        );
    }
}

/// TRIANGULATE: a failing search (log insert target removed → structural
/// 500) still increments the latency and SQL-op counters, privately.
#[tokio::test(flavor = "multi_thread")]
async fn failing_search_records_500_latency_and_sql_ops_privately() {
    let (pool, _db) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let app = spawn_app_with_metrics(pool.clone(), metrics.clone());

    // Remove the log target so the (successful) ranking pipeline still
    // issues its provider statements, then persistence fails → public 500.
    sqlx::query("DROP TABLE search_feedback, search_logs CASCADE")
        .execute(&pool)
        .await
        .expect("drop log tables to force the failure path");

    let (status, _) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto").await;
    assert_eq!(status, 500);

    assert!(
        metrics.request_count(SEARCH_ROUTE, 500) >= 1,
        "error-path latency counter recorded the failing request"
    );
    assert!(
        metrics.sql_ops_total(SEARCH_ROUTE) >= 2,
        "provider statements are counted even when the request fails"
    );
    for label in metrics.all_labels() {
        assert!(
            !label.contains(QUERY),
            "privacy: no metric label may carry the query text (found {label:?})"
        );
    }
}

/// The cache counters and the generation gauge exist from day one (stage 4
/// and stage 3 wire the real behaviors; the seams are already auditable).
#[test]
fn cache_and_generation_counters_start_at_their_boot_state() {
    let metrics = MemoryMetrics::new();
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 0);
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 0);
    assert_eq!(metrics.cache_total(CacheEvent::Eviction), 0);
    assert_eq!(metrics.generation_state(), GenerationState::NotLoaded);

    metrics.observe_cache(CacheEvent::Hit);
    metrics.observe_generation_state(GenerationState::Active);
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 1);
    assert_eq!(metrics.generation_state(), GenerationState::Active);
}
