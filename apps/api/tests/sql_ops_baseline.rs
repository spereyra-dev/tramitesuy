//! Task 2 TRIANGULATE: documents today's baseline `open` search-path cost —
//! a recorded number, NOT an assertion of correctness. Later slices drive
//! this number down (operations delta: cache-hit 1, new search ≤3,
//! intermediate `open` ≤4); this test pins what the optimization starts
//! from: 7 statements (FTS, trigram, selected-event lookup, top-event
//! lookup, log insert, event metadata, event procedures).

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn open_search_path_today_costs_seven_statements() {
    let (pool, counter) = fresh_migrated_counting_db().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app(pool);

    let ((status, body), count) = counter
        .measure(async {
            request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await
        })
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], "open", "the recorded path is the open one");

    assert_eq!(
        count, 7,
        "recorded baseline: FTS + trigram + selected-event lookup + top-event \
         lookup + log insert + event metadata + event procedures (observed {count})"
    );
}
