//! Task 1 (OPT-10, R14, operations-delta observability): the privacy-safe
//! metrics seam. A served `/api/v1/search` request increments the
//! search-latency and SQL-op counters, and no emitted label ever carries
//! the submitted query text, normalized text, or a cache-key fingerprint.

mod support;

use std::sync::Arc;

use api::metrics::{CacheEvent, GenerationState, MemoryMetrics, Metrics, OperationalAlert};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use support::*;
use tower::ServiceExt;

const QUERY: &str = "compre un auto";
const SEARCH_ROUTE: &str = "/api/v1/search";
const EVENTS_ROUTE: &str = "/api/v1/events/{slug}";

#[tokio::test(flavor = "multi_thread")]
async fn internal_metrics_exports_every_counter_as_prometheus_text() {
    let (pool, db_name) = fresh_migrated_db().await;
    let metrics = Arc::new(MemoryMetrics::new());
    metrics.observe_request(SEARCH_ROUTE, 500, 42);
    metrics.observe_sql_ops(SEARCH_ROUTE, 3);
    for event in [
        CacheEvent::Hit,
        CacheEvent::Miss,
        CacheEvent::Eviction,
        CacheEvent::Compute,
        CacheEvent::Grouped,
    ] {
        metrics.observe_cache(event);
    }
    metrics.observe_cache_size(2048, 2);
    metrics.observe_generation_state(GenerationState::Active);
    for alert in [
        OperationalAlert::PublicationLag,
        OperationalAlert::MemoryBudget,
        OperationalAlert::WarmingFailed,
    ] {
        metrics.observe_operational_alert(alert);
    }
    let app = spawn_app_with_metrics(pool, metrics);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let text = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec(),
    )
    .expect("utf-8");
    for line in [
        "tramitesuy_requests_total{route=\"/api/v1/search\",status=\"500\"} 1\n",
        "tramitesuy_request_latency_microseconds_total{route=\"/api/v1/search\",status=\"500\"} 42\n",
        "tramitesuy_sql_ops_total{route=\"/api/v1/search\"} 3\n",
        "tramitesuy_cache_events_total{event=\"hit\"} 1\n",
        "tramitesuy_cache_events_total{event=\"miss\"} 1\n",
        "tramitesuy_cache_events_total{event=\"eviction\"} 1\n",
        "tramitesuy_cache_events_total{event=\"compute\"} 1\n",
        "tramitesuy_cache_events_total{event=\"grouped\"} 1\n",
        "tramitesuy_cache_bytes 2048\n",
        "tramitesuy_cache_entries 2\n",
        "tramitesuy_generation_state{state=\"active\"} 1\n",
        "tramitesuy_operational_alerts_total{alert=\"publication_lag\"} 1\n",
        "tramitesuy_operational_alerts_total{alert=\"memory_budget\"} 1\n",
        "tramitesuy_operational_alerts_total{alert=\"warming_failed\"} 1\n",
    ] {
        assert!(text.contains(line), "missing {line:?} from {text}");
    }
    assert!(!text.contains("compre un auto"), "no query-derived labels");
    let (status, _) = request(&app, "GET", "/api/v1/metrics").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    common_drop(&db_name).await;
}

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

#[test]
fn prometheus_labels_are_escaped_and_unobserved_series_are_zero() {
    let metrics = MemoryMetrics::new();
    metrics.observe_request("/route/\"odd\\value\n", 200, 3);
    let text = metrics.render_prometheus();
    assert!(text.contains("route=\"/route/\\\"odd\\\\value\\n\",status=\"200\"} 1"));
    assert!(text.contains("tramitesuy_cache_events_total{event=\"hit\"} 0\n"));
    assert!(text.contains("tramitesuy_generation_state{state=\"not_loaded\"} 1\n"));
    assert!(text.contains("tramitesuy_generation_state{state=\"active\"} 0\n"));
}

#[test]
fn public_proxy_explicitly_denies_the_internal_metrics_path() {
    let config = include_str!("../../../docker/proxy/nginx.conf");
    let public_server = config
        .split("listen       443 ssl;")
        .nth(1)
        .expect("public HTTPS listener");
    assert!(public_server.contains("location = /metrics  { return 404; }"));
    assert!(public_server.contains("location = /ready    { return 404; }"));
    assert!(!public_server.contains("location /metrics { proxy_pass"));
}

/// F26: bounded, non-gating local measurement. This is a diagnostic, not a
/// performance assertion: run with `--nocapture` and compare on the same
/// machine. The fixture is intentionally small; do not extrapolate timings
/// or use a synthetic pool-saturation result to justify production changes.
#[tokio::test(flavor = "multi_thread")]
async fn measure_f26_candidates_without_changing_serving_behavior() {
    use std::hint::black_box;
    use std::time::Instant;

    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        limits_without_warming(),
    )
    .await;
    let uri = "/api/v1/search?q=compre%20un%20auto%20usado";
    assert_eq!(request(&app, "GET", uri).await.0, StatusCode::OK);
    assert_eq!(request(&app, "GET", uri).await.0, StatusCode::OK);
    let iterations = 32;
    let acquired = Instant::now();
    for _ in 0..iterations {
        black_box(pool.acquire().await.expect("pool connection"));
    }
    let acquire_us = acquired.elapsed().as_micros();
    let hit_start = Instant::now();
    for _ in 0..iterations {
        assert_eq!(request(&app, "GET", uri).await.0, StatusCode::OK);
    }
    let hit_us = hit_start.elapsed().as_micros();
    assert_eq!(metrics.cache_total(CacheEvent::Hit), iterations + 1);
    println!(
        "F26 acquire-before-hit: {iterations} pool acquires={acquire_us}us; {iterations} warm HTTP hits={hit_us}us (includes log insert, cache rebuild, JSON; acquire is a subset)"
    );

    let generation = state.active.load_full();
    let query = "compre un auto usado";
    let key = api::cache::CacheKey::new(
        generation.generation_id(),
        generation.engine_version().to_string(),
        query,
    );
    let entry = generation.cache.get(&key).expect("warm cache entry");
    let iterations = 1000;
    let baseline = Instant::now();
    for _ in 0..iterations {
        // Counterfactual only: clone the same ranked results/selection and
        // normalize, but do not copy tokens into each explanation. This is
        // not a candidate response; debug requires the original tokens.
        black_box(generation.engine.normalize(query));
        black_box(entry.results.clone());
        black_box(entry.selection.clone());
    }
    let without_tokens_us = baseline.elapsed().as_micros();
    let rebuild = Instant::now();
    for _ in 0..iterations {
        black_box(api::cache::rebuild(&generation.engine, query, &entry));
    }
    let rebuild_us = rebuild.elapsed().as_micros();
    println!(
        "F26 explanation rebuild: {iterations} actual={rebuild_us}us; no-explanation-token-copy counterfactual={without_tokens_us}us; results={} options={}",
        entry.results.len(),
        entry.selection.options.len()
    );

    // Measure the exact per-card existence query and a set-based equivalent
    // against this generation's immutable projections. Setup and build are
    // excluded; the gate also performs other checks (not timed here).
    let id = generation.generation_id();
    let cards: Vec<String> = sqlx::query_scalar(
        "SELECT jsonb_array_elements(c.cards) ->> 'slug' FROM generation_event_cards c WHERE generation_id = $1",
    ).bind(id).fetch_all(&pool).await.expect("projected cards");
    let iterations = 20;
    let per_card = Instant::now();
    let mut missing = 0usize;
    for _ in 0..iterations {
        for slug in &cards {
            if sqlx::query_scalar::<_, i32>(
                "SELECT 1 FROM generation_procedure_details WHERE generation_id = $1 AND slug = $2",
            )
            .bind(id)
            .bind(slug)
            .fetch_optional(&pool)
            .await
            .expect("detail lookup")
            .is_none()
            {
                missing += 1;
            }
        }
    }
    let per_card_us = per_card.elapsed().as_micros();
    let set_based = Instant::now();
    let mut set_missing = 0i64;
    for _ in 0..iterations {
        set_missing += sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM generation_event_cards c CROSS JOIN LATERAL jsonb_array_elements(c.cards) card WHERE c.generation_id = $1 AND NOT EXISTS (SELECT 1 FROM generation_procedure_details d WHERE d.generation_id = c.generation_id AND d.slug = card ->> 'slug')",
        ).bind(id).fetch_one(&pool).await.expect("set lookup");
    }
    let set_us = set_based.elapsed().as_micros();
    assert_eq!(missing as i64, set_missing);
    println!(
        "F26 relation existence: {} projected cards; {iterations} iterations per-card={}us ({} round trips), set-based={}us ({iterations} round trips); missing={missing}",
        cards.len(),
        per_card_us,
        cards.len() * iterations,
        set_us
    );
    drop(generation);
    drop(state);
    drop(app);
    drop(pool);
    common_drop(&db_name).await;
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
