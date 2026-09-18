//! Task 86 (API-9, D-3): the `POST /search/feedback` write path — the only
//! write endpoint of the MVP. A valid `{search_log_id, event_id, correct}`
//! body returns 201 and persists a `search_feedback` row linked to the
//! search log and the event; an unknown `search_log_id` or `event_id`
//! returns 400. No feedback UI exists anywhere (D-3: write path only).
//!
//! Integration tests run against the compose Postgres through the shared
//! scratch-database helper (`support`).

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn valid_feedback_returns_201_and_links_the_log_row() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    // A prior search produces the log row the feedback links to (the same
    // flow a real client follows: search → feedback).
    let (status, _) = request(&app, "GET", "/api/v1/search?q=compre+un+auto+usado").await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "the prior search succeeds"
    );

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

    let (status, body) = request_json(
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
    assert_eq!(
        status,
        axum::http::StatusCode::CREATED,
        "valid feedback returns 201"
    );

    let row: (sqlx::types::Uuid, sqlx::types::Uuid, bool) =
        sqlx::query_as("SELECT search_log_id, event_id, correct FROM search_feedback")
            .fetch_one(&pool)
            .await
            .expect("exactly one feedback row is persisted");
    assert_eq!(row.0, log_id, "the feedback row links the search log");
    assert_eq!(row.1, event_id, "the feedback row links the event");
    assert!(
        !row.2,
        "the feedback row carries the submitted correct flag"
    );

    assert_eq!(
        body,
        serde_json::json!({"status": "created"}),
        "the 201 body must be the exact public success shape"
    );
    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_search_log_id_returns_400() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let event_id: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM life_events WHERE slug = 'comprar-vehiculo'")
            .fetch_one(&pool)
            .await
            .expect("the seed fixture contains the event");
    let absent_log_id = "00000000-0000-0000-0000-000000000009"
        .parse::<sqlx::types::Uuid>()
        .expect("static absent uuid");

    let (status, body) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({
            "search_log_id": absent_log_id.to_string(),
            "event_id": event_id.to_string(),
            "correct": true
        }),
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "an unknown search_log_id must return 400"
    );
    assert_eq!(
        body,
        serde_json::json!({"error": "bad request"}),
        "the 400 body must be the exact public error shape"
    );
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM search_feedback")
        .fetch_one(&pool)
        .await
        .expect("feedback count readable");
    assert_eq!(stored, 0, "a rejected submission persists nothing");
    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_event_id_returns_400() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, _) = request(&app, "GET", "/api/v1/search?q=compre+un+auto+usado").await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let log_id: sqlx::types::Uuid =
        sqlx::query_scalar("SELECT id FROM search_logs ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("the search persisted a log row");
    let absent_event_id = "00000000-0000-0000-0000-000000000009"
        .parse::<sqlx::types::Uuid>()
        .expect("static absent uuid");

    let (status, body) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({
            "search_log_id": log_id.to_string(),
            "event_id": absent_event_id.to_string(),
            "correct": true
        }),
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "an unknown event_id must return 400"
    );
    assert_eq!(
        body,
        serde_json::json!({"error": "bad request"}),
        "the 400 body must be the exact public error shape"
    );
    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_or_incomplete_bodies_return_400() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    // Not JSON at all.
    let (status, body) = request_raw(&app, "POST", "/api/v1/search/feedback", b"not json").await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "a non-JSON body must return 400"
    );
    assert_eq!(
        body,
        serde_json::json!({"error": "bad request"}),
        "the 400 body must be the exact public error shape (no extractor echo)"
    );

    // Missing the `correct` field.
    let (status, _) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({"search_log_id": "00000000-0000-0000-0000-000000000009"}),
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "an incomplete body must return 400"
    );

    // A non-UUID search_log_id.
    let (status, _) = request_json(
        &app,
        "POST",
        "/api/v1/search/feedback",
        &serde_json::json!({"search_log_id": "not-a-uuid", "event_id": "00000000-0000-0000-0000-000000000009", "correct": true}),
    )
    .await;
    assert_eq!(
        status,
        axum::http::StatusCode::BAD_REQUEST,
        "a non-UUID id must return 400"
    );

    common_drop(&db_name).await;
}
