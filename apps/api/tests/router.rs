//! Task 70 (API-1): the router exposes exactly the seven `/api/v1` routes
//! from the api spec, unknown routes return 404, and `ApiError` responses
//! map to 404/400/500 while leaking no internals. The read endpoints'
//! behavioral contracts are tasks 71-73; the search endpoints dialed up in
//! C2 (tasks 79-82) and the feedback write path in C3 (task 86).
//!
//! Integration tests run against the compose Postgres through the shared
//! scratch-database helper (`support`).

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn the_seven_api_v1_routes_are_registered() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    for (method, uri) in [
        ("GET", "/api/v1/search?q=compre+un+auto"),
        ("GET", "/api/v1/search/debug?q=compre"),
        ("GET", "/api/v1/events/comprar-vehiculo"),
        ("GET", "/api/v1/categories"),
        ("GET", "/api/v1/categories/vehiculos/events"),
        ("GET", "/api/v1/procedures/4551"),
    ] {
        let (status, _) = request(&app, method, uri).await;
        assert_ne!(
            status,
            axum::http::StatusCode::NOT_FOUND,
            "{method} {uri} must be a registered /api/v1 route"
        );
    }

    let (status, _) = request(&app, "POST", "/api/v1/search/feedback").await;
    assert_ne!(
        status,
        axum::http::StatusCode::NOT_FOUND,
        "POST /api/v1/search/feedback must be a registered /api/v1 route"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_api_v1_routes_return_404() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    for uri in [
        "/api/v1/unknown",
        "/api/v1/events",
        "/api/v1/search/extra",
        "/api/v2/search",
        "/api/v1",
    ] {
        let (status, _) = request(&app, "GET", uri).await;
        assert_eq!(
            status,
            axum::http::StatusCode::NOT_FOUND,
            "GET {uri} is not part of the closed seven-endpoint inventory"
        );
    }

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_method_on_a_registered_route_is_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    // A 405 proves the route exists (method restriction), distinguishing a
    // registered route from an unknown one (404) — the inventory-closure
    // probe for the POST-only and GET-only endpoints.
    for (method, uri) in [
        ("GET", "/api/v1/search/feedback"),
        ("POST", "/api/v1/categories"),
        ("POST", "/api/v1/events/comprar-vehiculo"),
        ("POST", "/api/v1/search"),
    ] {
        let (status, _) = request(&app, method, uri).await;
        assert_eq!(
            status,
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "{method} {uri} must hit a registered route restricted to another method"
        );
    }

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn error_responses_leak_no_internals() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    // Unknown route: exact public body — no path, no query string, no
    // diagnostics (API-1 leak-none clause).
    let (status, body) = request(&app, "GET", "/api/v1/unknown?secret=leak-me").await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        serde_json::json!({"error": "not found"}),
        "the 404 body must be the exact public error shape"
    );

    // The feedback slot was dialed up in C3 (task 86): a malformed body
    // maps to the public 400 shape (no extractor echo); internal causes are
    // logged on the server, never serialized. (The search slot was dialed
    // up in C2 — task 79 — so it no longer serves a 500 probe.)
    let (status, body) = request_raw(&app, "POST", "/api/v1/search/feedback", b"not json").await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        serde_json::json!({"error": "bad request"}),
        "the 400 body must be the exact public error shape"
    );

    common_drop(&db_name).await;
}
