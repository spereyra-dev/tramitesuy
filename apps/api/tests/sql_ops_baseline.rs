//! Tasks 2/4/6/7: the recorded per-slice statement budgets — a recorded
//! number, NOT an assertion of correctness. Later slices drive these numbers
//! down (operations delta: cache-hit 1, new search ≤3, intermediate `open`
//! ≤4); each slice pins its budget here.
//!
//! After S2 task 6 (consolidated log insert — one statement resolving both
//! slugs) and task 7 (transition cards query replacing `by_event` per
//! search), the recorded paths are:
//! Open search path (4 statements): FTS, trigram, log insert (with
//! integrated slug resolution), dedicated cards query — the OPT-06
//! intermediate-phase ≤4 budget, held until the snapshot route lands.
//! Disambiguation / categories paths (3 / 3): FTS, trigram, consolidated
//! log insert.

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn open_search_path_costs_four_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "open", "the recorded path is the open one");
    assert_eq!(
        count, 4,
        "recorded budget after the transition cards query: FTS + trigram + \
         log insert + cards_by_event — the intermediate-phase ≤4 budget \
         (observed {count})"
    );
}

/// Recorded per-mode budget (task 4 baseline, task 6 consolidated): a
/// disambiguation search costs 3 statements (2 providers + the consolidated
/// log insert with integrated slug resolution).
#[tokio::test(flavor = "multi_thread")]
async fn disambiguation_path_costs_three_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    let app = spawn_app(pool);

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=auto").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "disambiguation");
    assert_eq!(
        count, 3,
        "recorded budget after the consolidated log insert: FTS + trigram + \
         log insert (observed {count})"
    );
}

/// Recorded per-mode budget (task 4 baseline, unchanged by task 6's
/// consolidation): a categories-mode search costs 3 statements (2 providers
/// + the single-statement log insert; no event ids to resolve).
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
