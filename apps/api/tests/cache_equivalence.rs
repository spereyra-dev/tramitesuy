//! S9 task 28 (RED): cached entries hold reusable computational results and
//! every response is REBUILT for the current request (OPT-05, search-engine
//! delta "Cached and uncached results are identical", R2/R6). Cached vs.
//! uncached responses are identical for the same generation and input
//! across /search, /search/debug, accents, synonyms, zero-match inputs, and
//! redaction-requiring inputs; structural errors are never cached; feedback
//! responses are never cached; and synonym variants never exchange text or
//! tokens (they are separate misses — separate keys).

mod support;

use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";
const DEBUG: &str = "/api/v1/search/debug";

#[tokio::test(flavor = "multi_thread")]
async fn cached_and_uncached_search_responses_are_identical() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    let (first_status, first) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    let (second_status, second) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;

    assert_eq!(first_status, axum::http::StatusCode::OK);
    assert_eq!(second_status, axum::http::StatusCode::OK);
    assert_eq!(
        first, second,
        "the cached response is identical to the uncached one for the same input"
    );

    // Evidence that the second response came from the cache: exactly one
    // miss (the first request) and one hit (the second).
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 1);
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 1);

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_and_uncached_debug_responses_are_identical() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    let (first_status, first) = request(
        &app,
        "GET",
        &format!("{DEBUG}?q=compre%20un%20auto%20usado"),
    )
    .await;
    let (second_status, second) = request(
        &app,
        "GET",
        &format!("{DEBUG}?q=compre%20un%20auto%20usado"),
    )
    .await;

    assert_eq!(first_status, axum::http::StatusCode::OK);
    assert_eq!(second_status, axum::http::StatusCode::OK);
    assert_eq!(
        first, second,
        "cached and uncached debug payloads — tokens and explanations \
         included — are identical for the same request"
    );
    // TRIANGULATE (task 28): the debug TOKEN lists of the cached and the
    // uncached response are identical, and the echo belongs to the request
    // itself (never another request's text).
    assert_eq!(
        first["tokens"], second["tokens"],
        "the debug token list is freshly built and identical for the same request"
    );
    assert_eq!(first["query"], "compre un auto usado");
    assert_eq!(first["normalized_query"], "compre auto usado");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accented_query_is_served_identically_from_the_cache() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    // The same ACCENTED effective input twice: byte-identical raw q bytes
    // ⇒ same fingerprint ⇒ hit. (A DIFFERENT accent variant is a different
    // fingerprint by task 27's key rule — never collapsed.)
    let uri = format!("{SEARCH}?q=compr%C3%A9%20un%20auto%20usado");
    let (first_status, first) = request(&app, "GET", &uri).await;
    let (second_status, second) = request(&app, "GET", &uri).await;

    assert_eq!(first_status, axum::http::StatusCode::OK);
    assert_eq!(second_status, axum::http::StatusCode::OK);
    assert_eq!(first, second, "the accented hit is byte-identical");
    assert_eq!(first["query"], "compré un auto usado");
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 1);

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn synonym_variants_are_separate_misses_that_never_exchange_text() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    // `compré un auto` and `compre un coche` share canonical tokens through
    // the synonym map, but their effective engine input differs: two
    // separate keys, two misses, two cache entries (task 27 TRIANGULATE,
    // served end-to-end here).
    let (status_a1, a1) = request(&app, "GET", &format!("{DEBUG}?q=compr%C3%A9%20un%20auto")).await;
    let (status_b1, b1) = request(&app, "GET", &format!("{DEBUG}?q=compre%20un%20coche")).await;
    assert_eq!(status_a1, axum::http::StatusCode::OK);
    assert_eq!(status_b1, axum::http::StatusCode::OK);

    let generation = state.active.load_full();
    assert_eq!(
        generation.cache.entry_count(),
        2,
        "the two variants occupy SEPARATE cache entries (no collapse on \
         canonical tokens)"
    );

    // Each variant hits its OWN entry on repeat, and the echoes never
    // exchange text: a1's repeat echoes `compré un auto`, b1's echoes
    // `compre un coche`.
    let (_, a2) = request(&app, "GET", &format!("{DEBUG}?q=compr%C3%A9%20un%20auto")).await;
    let (_, b2) = request(&app, "GET", &format!("{DEBUG}?q=compre%20un%20coche")).await;
    assert_eq!(a1, a2, "a's cached response equals a's uncached response");
    assert_eq!(b1, b2, "b's cached response equals b's uncached response");
    assert_eq!(a2["query"], "compré un auto");
    assert_eq!(b2["query"], "compre un coche");
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 2);
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 2);

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_zero_match_query_is_served_identically_from_the_cache() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    let (first_status, first) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=xyzabc%20sin%20coincidencia"),
    )
    .await;
    let (second_status, second) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=xyzabc%20sin%20coincidencia"),
    )
    .await;

    assert_eq!(first_status, axum::http::StatusCode::OK);
    assert_eq!(second_status, axum::http::StatusCode::OK);
    assert_eq!(
        first, second,
        "the zero-match (categories fallback) result is cached and served \
         identically"
    );
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 1);

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redaction_requiring_input_is_served_identically_from_the_cache() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    // A cédula-shaped input: the search runs on the raw text (both
    // responses echo it), while the persisted log is redacted. The cached
    // and uncached responses must be identical — and the cache must not
    // have retained the document number.
    let uri = format!("{SEARCH}?q=compre%20un%20auto%204.123.456-7");
    let (first_status, first) = request(&app, "GET", &uri).await;
    let (second_status, second) = request(&app, "GET", &uri).await;

    assert_eq!(first_status, axum::http::StatusCode::OK);
    assert_eq!(second_status, axum::http::StatusCode::OK);
    assert_eq!(first, second, "redaction-requiring input: hit == miss");
    assert_eq!(first["query"], "compre un auto 4.123.456-7");
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 1);

    // The persisted log carries the REDACTED text only (R2 boundary).
    let (redacted_rows, raw_rows): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE query LIKE '%<REDACTED>%'), \
                count(*) FILTER (WHERE query LIKE '%4.123.456-7%') \
         FROM search_logs",
    )
    .fetch_one(&pool)
    .await
    .expect("search_logs readable");
    assert!(redacted_rows >= 1, "the log row is redacted");
    assert_eq!(raw_rows, 0, "the raw document number never reaches the log");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn structural_errors_are_not_cached() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    // Force a structural failure on the log path: with `search_logs` gone
    // (CASCADE also removes the feedback table — this scratch database is
    // single-purpose), every search ends in the public structural 500 AFTER
    // the computation. Audited: fixed table name, never input.
    sqlx::query("DROP TABLE search_logs CASCADE")
        .execute(&pool)
        .await
        .expect("search_logs dropped");

    let (status, _body) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);

    let generation = state.active.load_full();
    assert_eq!(
        generation.cache.entry_count(),
        0,
        "a search that ends in a structural error caches nothing"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn feedback_responses_are_never_cached() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = std::sync::Arc::new(MemoryMetrics::new());
    let (app, state) = spawn_app_with_state_and_metrics(pool.clone(), metrics.clone());

    // One search warms exactly one entry and produces the log row a valid
    // feedback links to; the write-path request must not add or touch
    // anything in the cache.
    let (_, _) = request(
        &app,
        "GET",
        &format!("{SEARCH}?q=compre%20un%20auto%20usado"),
    )
    .await;
    let log_id: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM search_logs ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("the search persisted a log row");
    let event_id: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM life_events WHERE slug = 'comprar-vehiculo'")
            .fetch_one(&pool)
            .await
            .expect("the seed fixture contains the event");
    let (feedback_status, _) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({
            "search_log_id": log_id.to_string(),
            "event_id": event_id.to_string(),
            "correct": false
        }),
    )
    .await;
    assert_eq!(feedback_status, axum::http::StatusCode::CREATED);

    let generation = state.active.load_full();
    assert_eq!(
        generation.cache.entry_count(),
        1,
        "the write path (feedback) never caches and never mutates the cache"
    );

    common_drop(&db_name).await;
}

#[test]
fn an_engine_version_change_invalidates_earlier_keys() {
    // TRIANGULATE (task 28): a taxonomy/engine version change invalidates
    // earlier keys — a key computed under E1 is never reused under E2.
    let cache = api::cache::SearchCache::new(api::cache::CacheLimits::default());
    let generation = uuid::Uuid::now_v7();
    cache.insert(
        api::cache::CacheKey::new(generation, "search-rules-v1".to_string(), "compre un auto"),
        entry_placeholder(),
    );
    assert!(
        cache
            .get(&api::cache::CacheKey::new(
                generation,
                "search-rules-v2".to_string(),
                "compre un auto",
            ))
            .is_none(),
        "the E1 key cannot serve an E2 request"
    );
    assert!(
        cache
            .get(&api::cache::CacheKey::new(
                generation,
                "search-rules-v1".to_string(),
                "compre un auto",
            ))
            .is_some(),
        "the E1 key still serves its own version"
    );
}

/// A minimal valid cached entry (any query works — the key only hashes the
/// effective text).
fn entry_placeholder() -> api::cache::CachedEntry {
    api::cache::CachedEntry {
        candidates: Vec::new(),
        results: Vec::new(),
        confidence: 0.0,
        selection: search::types::Selection {
            mode: search::types::SelectionMode::Categories,
            event_slug: None,
            options: Vec::new(),
            categories: Vec::new(),
        },
    }
}
