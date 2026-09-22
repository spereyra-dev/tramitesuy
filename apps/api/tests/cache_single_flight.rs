//! S10 task 29 (RED): single-flight with bounded wait (OPT-05/OPT-09,
//! search-cache delta "Single-flight grouping with bounded wait", R7).
//!
//! The first miss computes; concurrent identical keys clone the in-flight
//! holder and wait within the remaining deadline budget (a waiter whose
//! window elapses recomputes on its own account, never hangs); the result
//! enters the cache for its own generation only; and every request —
//! leader, waiter or later hit — persists its own log. 100 identical
//! concurrent requests produce exactly 1 ranking computation, 100
//! successful responses, and 100 persisted log rows; two DIFFERENT keys
//! compute concurrently without grouping.

mod support;

use std::sync::Arc;
use std::time::Duration;

use api::config::ApiLimits;
use api::metrics::{CacheEvent, MemoryMetrics};
use support::*;

const SEARCH: &str = "/api/v1/search";

/// Barrier: waits until `count` requests have passed the cache-miss point.
/// The leader is already blocked on the locked FTS table (see the tests
/// below) and every other request has already decided — atomically, inside
/// `join_or_lead` — between leading and waiting, so the grouping outcome is
/// deterministic instead of a poll race.
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
async fn hundred_identical_concurrent_requests_share_one_computation() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        // S12 admission default is 32; this test drives 100 concurrent
        // requests, so its budget is raised to admit all of them (the
        // limit under test here is single-flight, not admission).
        ApiLimits {
            max_concurrent_searches: 100,
            ..limits_without_warming()
        },
    )
    .await;

    // Deterministic grouping barrier: lock the FTS table so the LEADER's
    // first candidate query blocks while every identical request joins the
    // in-flight holder. Nothing can compute through the lock, so the
    // leader/waiter split is fixed before any result exists.
    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

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
    wait_for_misses(&metrics, 100).await;
    lock_tx.commit().await.expect("release the barrier");

    for handle in handles {
        let (status, body) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(body["mode"], "open");
        assert_eq!(body["results"][0]["event"]["slug"], "comprar-vehiculo");
    }

    // Exactly ONE ranking computation ran: the leader. The 99 others were
    // served by the shared computation (they were never misses that
    // computed on their own account).
    assert_eq!(
        metrics.cache_total(CacheEvent::Compute),
        1,
        "100 identical concurrent requests produce exactly 1 ranking computation"
    );
    assert_eq!(
        metrics.cache_total(CacheEvent::Grouped),
        99,
        "the 99 non-leading requests were served by the shared computation"
    );
    assert_eq!(
        metrics.cache_total(CacheEvent::Hit),
        0,
        "no request was served from the cache: the group computed once"
    );

    // Every request persists its OWN log: 100 requests, 100 rows.
    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(&pool)
        .await
        .expect("search_logs readable");
    assert_eq!(logs, 100, "each grouped request persists its own log row");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiter_whose_window_elapses_recomputes_instead_of_hanging() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        // A 100 ms search deadline IS the wait-window budget: the leader is
        // pinned behind the lock past it, so the waiter must give up and
        // compute on its own account instead of hanging forever.
        ApiLimits {
            search_deadline: Duration::from_millis(100),
            ..limits_without_warming()
        },
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

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
    wait_for_misses(&metrics, 2).await;
    // Hold the lock past the 100 ms window: the waiter's window elapses
    // while the leader is still computing.
    tokio::time::sleep(Duration::from_millis(300)).await;
    lock_tx.commit().await.expect("release the barrier");

    for handle in handles {
        let (status, _) = handle.await.expect("request task completes");
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "the expired waiter recomputes and succeeds within its deadline"
        );
    }

    // Two computations: the leader's, plus the expired waiter's own. The
    // waiter was NOT served by the group.
    assert_eq!(
        metrics.cache_total(CacheEvent::Compute),
        2,
        "the expired waiter recomputed on its own account"
    );
    assert_eq!(
        metrics.cache_total(CacheEvent::Grouped),
        0,
        "the expired waiter was never served by the group"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn two_different_keys_compute_concurrently_without_grouping() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let metrics = Arc::new(MemoryMetrics::new());
    let (app, _state) = spawn_app_with_generation_state_and_metrics(
        pool.clone(),
        metrics.clone(),
        limits_without_warming(),
    )
    .await;

    let mut lock_tx = pool.begin().await.expect("barrier transaction");
    sqlx::query("LOCK TABLE life_events IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock_tx)
        .await
        .expect("lock the FTS table");

    // TRIANGULATE (task 29): two DIFFERENT keys (different fingerprints —
    // one matches the fixture event, one matches nothing) must compute
    // concurrently, each its own group.
    let first = {
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
    let second = {
        let app = app.clone();
        tokio::spawn(async move {
            request(
                &app,
                "GET",
                &format!("{SEARCH}?q=xyzabc%20sin%20coincidencia"),
            )
            .await
        })
    };
    wait_for_misses(&metrics, 2).await;
    lock_tx.commit().await.expect("release the barrier");

    for handle in [first, second] {
        let (status, _) = handle.await.expect("request task completes");
        assert_eq!(status, axum::http::StatusCode::OK);
    }

    assert_eq!(
        metrics.cache_total(CacheEvent::Compute),
        2,
        "two different keys compute independently (no grouping)"
    );
    assert_eq!(
        metrics.cache_total(CacheEvent::Grouped),
        0,
        "different keys never share a computation"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wait_window_never_exceeds_the_deadline_budget() {
    let cache = api::cache::SearchCache::new(api::cache::CacheLimits::default());
    let key = api::cache::CacheKey::new(
        uuid::Uuid::now_v7(),
        "search-rules-v1".to_string(),
        "compre un auto",
    );

    let leader = match cache.join_or_lead(&key) {
        api::cache::Flight::Lead(publisher) => publisher,
        _ => panic!("the first miss leads the computation"),
    };
    let waiter = match cache.join_or_lead(&key) {
        api::cache::Flight::Wait(handle) => handle,
        _ => panic!("an identical key joins the in-flight computation"),
    };

    let started = std::time::Instant::now();
    let outcome = waiter.wait(Duration::from_millis(50)).await;
    let elapsed = started.elapsed();
    assert!(
        outcome.is_none(),
        "the leader never published within the window, so the waiter expired"
    );
    assert!(
        elapsed >= Duration::from_millis(50),
        "the wait respects the full window"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the wait never exceeds the window budget (no unbounded hang)"
    );

    // The abandoned leader must release the in-flight holder, so a later
    // identical request computes fresh instead of waiting forever.
    drop(leader);
    assert!(
        matches!(cache.join_or_lead(&key), api::cache::Flight::Lead(_)),
        "the abandoned holder released the key"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn waiters_receive_the_leaders_published_result() {
    let cache = api::cache::SearchCache::new(api::cache::CacheLimits::default());
    let key = api::cache::CacheKey::new(
        uuid::Uuid::now_v7(),
        "search-rules-v1".to_string(),
        "compre un auto",
    );

    let leader = match cache.join_or_lead(&key) {
        api::cache::Flight::Lead(publisher) => publisher,
        _ => panic!("the first miss leads the computation"),
    };
    let waiter = match cache.join_or_lead(&key) {
        api::cache::Flight::Wait(handle) => handle,
        _ => panic!("an identical key joins the in-flight computation"),
    };

    leader.publish(api::cache::SharedOutcome::Computed(Arc::new(
        entry_placeholder(),
    )));

    let outcome = waiter.wait(Duration::from_secs(1)).await;
    match outcome {
        Some(shared) => match &*shared {
            api::cache::SharedOutcome::Computed(entry) => {
                assert_eq!(entry.byte_size(), entry_placeholder().byte_size());
            }
            other => panic!("the waiter received the published computation: {other:?}"),
        },
        None => panic!("the waiter receives the leader's published result"),
    }

    // Publishing is NOT caching: the handler commits the cache insert only
    // after its own log persists, so a fresh key after the publish is a new
    // leader, not a stale join.
    assert!(
        matches!(cache.join_or_lead(&key), api::cache::Flight::Lead(_)),
        "publishing releases the in-flight holder; the cache commit is the handler's"
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
