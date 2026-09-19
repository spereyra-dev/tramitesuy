//! Tasks 2/4: the recorded current-behavior baselines — a recorded number,
//! NOT an assertion of correctness. Later slices drive these numbers down
//! (operations delta: cache-hit 1, new search ≤3, intermediate `open` ≤4);
//! these tests pin what the optimization starts from.
//!
//! Open search path (7 statements): FTS, trigram, selected-event lookup,
//! top-event lookup, log insert, event metadata, event procedures.
//! Disambiguation / categories paths (4 / 3): FTS, trigram, log insert,
//! plus the top-event lookup when a top event exists.

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn open_search_path_today_costs_seven_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "open", "the recorded path is the open one");
    assert_eq!(
        count, 7,
        "recorded baseline: FTS + trigram + selected-event lookup + top-event \
         lookup + log insert + event metadata + event procedures (observed {count})"
    );
}

/// Recorded per-mode baseline (task 4): a disambiguation search costs 4
/// statements (2 providers + log insert + top-event lookup).
#[tokio::test(flavor = "multi_thread")]
async fn disambiguation_path_today_costs_four_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=auto").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "disambiguation");
    assert_eq!(
        count, 4,
        "recorded baseline: FTS + trigram + log insert + top-event lookup \
         (observed {count})"
    );
}

/// Recorded per-mode baseline (task 4): a categories-mode search costs 3
/// statements (2 providers + log insert; no event ids to resolve).
#[tokio::test(flavor = "multi_thread")]
async fn categories_path_today_costs_three_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/search?q=quiero%20abrir%20una%20cuenta%20bancaria",
    )
    .await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "categories");
    assert_eq!(
        count, 3,
        "recorded baseline: FTS + trigram + log insert (observed {count})"
    );
}
