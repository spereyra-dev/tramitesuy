//! S14 task 45 (spec §7 test 7, OPT-02/OPT-03): database-down behavior
//! with a warm cache. The generation snapshot is loaded in memory at
//! boot, so a dead durable store must keep every snapshot read working
//! (categories, events, procedures — all zero-SQL) while the search and
//! feedback routes fail on their durable dependencies:
//!
//! - search cannot deliver a cached OR freshly computed result without
//!   persisting its consolidated log FIRST (log-before-respond): with the
//!   database down the log insert fails, and the failure is a structural
//!   5xx — never a wrong success, never a partial ranking;
//! - feedback cannot persist its row at all — the same structural
//!   failure.
//!
//! The database is taken down for real: the scratch database's backends
//! are terminated and new connections are disallowed
//! (`ALLOW_CONNECTIONS = false`), so the API's pool can neither reuse
//! nor acquire. Which 5xx shape a failing search surfaces depends on
//! where the failure is observed: a terminated pooled connection dies
//! with `admin_shutdown` (structural 500), while a pool that cannot
//! acquire within the 500 ms timeout surfaces sqlx's `PoolTimedOut`,
//! which the search error contract maps to the SAME 503 + `Retry-After`
//! overload shape (pinned by `apps/api/tests/deadline.rs`). Both are
//! structural failures of the durable dependency; the invariant this
//! suite pins is "no success without the durable dependency".

mod support;

use axum::Router;
use axum::http::StatusCode;
use sqlx::postgres::PgPoolOptions;
use support::*;

fn admin_url() -> String {
    std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
}

/// Audited: names are generated internally (`c1_<pid>_<nanos>`), never
/// from user input; sqlx 0.9 requires an explicit safety assertion for
/// dynamic SQL.
fn audited(sql: String) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(sql)
}

/// Boots the API over a scratch database with the production acquire
/// timeout (500 ms), warms the search cache with one real request (its
/// response is asserted to be a computed open search), and then takes
/// the database down for real. Returns the router and the scratch name.
async fn boot_warm_then_take_db_down() -> (Router, String) {
    let (pool, name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    drop(pool);

    // Rebuild the pool with the production acquire timeout so a dead
    // database fails within the documented window (the support helper's
    // default is sqlx's 30 s, which would mask the contract).
    let base = admin_url();
    let url = format!("{}/{}", base.trim_end_matches("/postgres"), name);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_millis(500))
        .test_before_acquire(false)
        .connect(&url)
        .await
        .expect("pool with the production acquire timeout");

    let app = spawn_app_with_generation(pool.clone()).await;

    // Warm the cache while the database is still up: one real request
    // computes, responds 200, and leaves the entry in the generation's
    // cache — the durable dependency is proven hot before it goes down.
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["mode"], "open", "the warm entry is a computed search");

    // The generation is loaded; drop the pool handle (the app keeps its
    // own Arc) before the database goes away.
    drop(pool);

    take_db_down(&name).await;
    (app, name)
}

/// Terminates every live backend of the scratch database and disallows
/// new connections: the API's pool can neither reuse nor acquire.
async fn take_db_down(name: &str) {
    let admin = sqlx::PgPool::connect(&admin_url())
        .await
        .expect("admin pool");
    sqlx::query(audited(format!(
        "ALTER DATABASE {name} ALLOW_CONNECTIONS false"
    )))
    .execute(&admin)
    .await
    .expect("disallow new connections to the scratch database");
    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = $1 AND pid <> pg_backend_pid()",
    )
    .bind(name)
    .execute(&admin)
    .await
    .expect("terminate the scratch database's backends");
}

/// Restores connections so the scratch database can be dropped at the end.
async fn restore_connections(name: &str) {
    let admin = sqlx::PgPool::connect(&admin_url())
        .await
        .expect("admin pool for cleanup");
    sqlx::query(audited(format!(
        "ALTER DATABASE {name} ALLOW_CONNECTIONS true"
    )))
    .execute(&admin)
    .await
    .expect("re-allow connections for cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_database_down_snapshot_reads_serve_and_search_feedback_fail() {
    let (app, name) = boot_warm_then_take_db_down().await;

    // The repeated query below is the SAME warmed key from the boot
    // helper: a cache HIT that must still fail on the durable log
    // dependency.
    //
    // Snapshot reads keep working (zero SQL, all in memory).
    for uri in [
        "/api/v1/categories",
        "/api/v1/events/comprar-vehiculo",
        "/api/v1/procedures/4551",
    ] {
        let (status, body) = request(&app, "GET", uri).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the snapshot read {uri} must survive the database being down: {body}"
        );
        assert!(
            !body.is_null(),
            "the snapshot read {uri} answers with real snapshot data"
        );
    }

    // A search — even a cache HIT for the repeated query — fails on its
    // durable dependency: the log must persist BEFORE the response.
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert!(
        status.is_server_error(),
        "a cache-hit search still fails on its durable log dependency when \
         the database is down — never a success: {status} {body}"
    );
    assert_eq!(
        body["error"], "internal server error",
        "no internal detail \
        leaks: {body}"
    );

    // Feedback fails on its durable dependency: the row cannot be
    // persisted (the failure is not an FK violation, so it keeps the
    // structural 500, no internal detail).
    let (status, body) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({
            "search_log_id": "00000000-0000-0000-0000-000000000001",
            "event_id": "00000000-0000-0000-0000-000000000002",
            "correct": true
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "feedback fails on its durable persistence dependency when the \
         database is down: {body}"
    );
    assert_eq!(
        body["error"], "internal server error",
        "no internal detail leaks: {body}"
    );

    restore_connections(&name).await;
    common_drop(&name).await;
}

/// The warm-cache leg of the main test as its own probe: the search cache
/// is warmed BEFORE the database goes down (the warm-up response is
/// asserted to be a computed 200), then the repeated query must still
/// fail — log-before-respond makes the durable store a dependency even
/// for a fully cached computation.
#[tokio::test(flavor = "multi_thread")]
async fn a_warm_cache_hit_still_fails_when_the_log_cannot_persist() {
    let (app, name) = boot_warm_then_take_db_down().await;

    // The cache HIT (same key the boot helper warmed) fails on its
    // durable log dependency.
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert!(
        status.is_server_error(),
        "even a warm cache-hit search fails when its log cannot persist — \
         never a wrong success: {status} {body}"
    );
    assert_eq!(body["error"], "internal server error", "{body}");

    restore_connections(&name).await;
    common_drop(&name).await;
}
