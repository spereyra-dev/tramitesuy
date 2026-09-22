//! S10 task 33 (RED): cache observability (OPT-05/OPT-10, operations delta
//! "privacy-safe observability", R14). Hits/misses/evictions/bytes/entries/
//! grouped computations surface as counters — and NO label, trace, or
//! access-log field ever carries the query text, its normalized form, or
//! the cache-key fingerprint, on the success path AND the error path.

mod support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;

use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";
const QUERY_A: &str = "compre%20un%20auto%20usado";
const QUERY_B: &str = "vender%20mi%20rodado%20usado";
const QUERY_C: &str = "xyzabc%20sin%20coincidencia";

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
async fn cache_counters_match_observed_behavior_without_query_labels() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        pool.clone(),
        &repo_root().join("data"),
        // A one-entry cache so a second query provably EVICTS the first.
        api::config::ApiLimits {
            cache: api::cache::CacheLimits {
                max_entries: 1,
                ..api::cache::CacheLimits::default()
            },
            ..limits_without_warming()
        },
        metrics.clone(),
    )
    .await
    .expect("boot the state");
    let app = api::build_router(state.clone());

    // A: miss + compute, then hit. B: miss + compute (evicting A), then hit.
    let (status, _) = request(&app, "GET", &format!("{SEARCH}?q={QUERY_A}")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(&app, "GET", &format!("{SEARCH}?q={QUERY_A}")).await;
    assert_eq!(status, StatusCode::OK, "query A hit");
    let (status, _) = request(&app, "GET", &format!("{SEARCH}?q={QUERY_B}")).await;
    assert_eq!(status, StatusCode::OK, "query B computed (evicting A)");
    let (status, _) = request(&app, "GET", &format!("{SEARCH}?q={QUERY_B}")).await;
    assert_eq!(status, StatusCode::OK, "query B hit after A was evicted");

    // A grouped computation: two concurrent identical requests share one
    // computation (the first misses and leads; the second joins). The FTS
    // lock pins the leader mid-computation so the grouping is
    // deterministic.
    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(
                async move { request(&app, "GET", &format!("{SEARCH}?q={QUERY_C}")).await },
            )
        })
        .collect();
    wait_for_misses(&metrics, 4).await;
    lock_tx.commit().await.expect("release the barrier");
    for handle in handles {
        let (status, _) = handle.await.expect("request task completes");
        assert_eq!(status, StatusCode::OK);
    }

    // The counters match the observed behavior exactly: 2 hits (A, B), 4
    // misses (A, B, and both C requests), 2 computations (A, B) plus the
    // group's leader (C) = 3, one grouped request (the C waiter), and TWO
    // evictions (the one-entry limit evicts A when B inserts, B when the
    // C leader inserts).
    assert_eq!(metrics.cache_total(CacheEvent::Hit), 2);
    assert_eq!(metrics.cache_total(CacheEvent::Miss), 4);
    assert_eq!(metrics.cache_total(CacheEvent::Compute), 3);
    assert_eq!(metrics.cache_total(CacheEvent::Grouped), 1);
    assert_eq!(
        metrics.cache_total(CacheEvent::Eviction),
        2,
        "each new insert under the one-entry limit evicts the previous          entry (A on B's insert, B on C's insert)"
    );

    // The size gauges track the live cache state.
    let generation = state.active.load_full();
    assert_eq!(
        metrics.cache_entries(),
        generation.cache.entry_count(),
        "the entries gauge tracks the live cache"
    );
    assert_eq!(
        metrics.cache_bytes(),
        generation.cache.bytes(),
        "the bytes gauge tracks the live cache accounting"
    );

    // Privacy (R14): no label carries the query text, its normalized
    // form, or the cache key's fingerprint (hex or raw bytes).
    let leaked = query_fragments()
        .into_iter()
        .chain(fingerprint_hexes())
        .any(|needle| {
            metrics
                .all_labels()
                .iter()
                .any(|label| label.contains(&needle))
        });
    assert!(
        !leaked,
        "no metric label carries query text, normalized text, or the key \
         fingerprint: {:?}",
        metrics.all_labels()
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_search_emits_no_query_text_on_the_error_path() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let state = api::state::AppState::boot_with_metrics(
        pool.clone(),
        &repo_root().join("data"),
        limits_without_warming(),
        metrics.clone(),
    )
    .await
    .expect("boot the state");
    let app = api::build_router(state.clone());

    // Force the structural failure: with `search_logs` gone every search
    // ends in the public 500 AFTER its computation (this scratch database
    // is single-purpose; CASCADE also removes the feedback table).
    sqlx::query("DROP TABLE search_logs CASCADE")
        .execute(&pool)
        .await
        .expect("search_logs dropped");

    let (status, body) = request(&app, "GET", &format!("{SEARCH}?q={QUERY_A}")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // TRIANGULATE (task 33): the error path is openapi-independent and
    // emits no query text — not in a metric label, not in the public
    // error body (the leak-none clause: internal detail never serializes,
    // and the error message names the SQL failure, never the query).
    let leaked = query_fragments()
        .into_iter()
        .chain(fingerprint_hexes())
        .any(|needle| {
            metrics
                .all_labels()
                .iter()
                .any(|label| label.contains(&needle))
                || body
                    .to_string()
                    .to_lowercase()
                    .contains(&needle.to_lowercase())
        });
    assert!(
        !leaked,
        "a failing search emits no query text in any label or log field: \
         labels {:?}, body {body}",
        metrics.all_labels()
    );

    common_drop(&db_name).await;
}

/// The query fragments that must NEVER appear in a label or log field:
/// the raw text, its normalized form, and the engine's canonical tokens.
fn query_fragments() -> Vec<String> {
    vec![
        "compre un auto".to_string(),
        "compre auto".to_string(),
        "comprar vehiculo".to_string(),
        "vender mi rodado".to_string(),
        "xyzabc".to_string(),
        "compro una casa".to_string(),
    ]
}

/// The hex renderings of the fingerprints the tests exercised — no label
/// may carry a cache-key fingerprint in ANY form.
fn fingerprint_hexes() -> Vec<String> {
    [
        "compre un auto usado",
        "vender mi rodado usado",
        "xyzabc sin coincidencia",
    ]
    .iter()
    .map(|effective| hex(&api::cache::fingerprint(effective)))
    .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
