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
//! After S7 task 21 (snapshot route): the open path with a loaded
//! generation still records 4 statements — FTS, the generation-scoped
//! trigram provider's transaction-local threshold, the precomputed surface
//! query, and the consolidated log — because the cards now cost 0 (snapshot)
//! while the generation trigram provider costs one extra `set_config`
//! statement (design §2.2). S8 consolidates the provider statement budget.
//! Disambiguation / categories paths (3 / 3 cold): FTS, trigram, consolidated
//! log insert (no-snapshot path; unchanged by S7).

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

/// S7 task 21 (RED first): with a loaded generation snapshot the open path
/// serves its cards from the snapshot — `cards_by_event` is no longer
/// executed (proven by dropping the relations table: the response still
/// carries the fixture cards). The recorded budget is exactly 5 traced
/// statements: FTS (1) + the generation trigram provider (3: the traced
/// transaction BEGIN + the transaction-local `set_config` threshold +
/// the precomputed-surface SELECT; COMMIT is not traced) + the consolidated
/// log insert (1). Real data statements = 4, matching the intermediate-phase
/// ≤4 budget with snapshot cards; the traced transaction ceremony is the +1
/// the S8 provider consolidation must absorb to reach the final ≤3.
#[tokio::test(flavor = "multi_thread")]
async fn open_search_with_snapshot_cards_costs_five_traced_statements() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    // The snapshot route is verified: serving no longer touches the legacy
    // relation table.
    sqlx::query("DROP TABLE life_event_procedures CASCADE")
        .execute(&pool)
        .await
        .expect("drop the transition cards source");

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["mode"], "open", "the recorded path is the open one");
    let procedures = body["results"][0]["procedures"]
        .as_array()
        .expect("cards served");
    assert_eq!(
        procedures
            .iter()
            .map(|card| card["external_id"].as_str().expect("external_id"))
            .collect::<Vec<_>>(),
        vec!["4551", "2368"],
        "the open cards are served from the loaded snapshot, not the \
         dropped legacy relation table: {body}"
    );
    assert_eq!(
        count, 5,
        "recorded budget with snapshot cards: FTS + trigram provider \
         (BEGIN + GUC + surface) + consolidated log (observed {count})"
    );
}

/// S7 task 21 TRIANGULATE: `/search/debug` uses the same captured
/// generation (the generation-scoped provider statements) and still
/// persists its log row before responding.
#[tokio::test(flavor = "multi_thread")]
async fn debug_with_snapshot_uses_the_captured_generation_and_logs() {
    let (pool, section) = fresh_counting_db_section().await;
    seed_search_fixture(&pool).await;
    publish_sample_generation(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    section.reset();
    let (status, body) = request(&app, "GET", "/api/v1/search/debug?q=compre%20un%20coche").await;
    let count = section.count();

    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["results"][0]["slug"], "comprar-vehiculo");
    assert_eq!(
        count, 5,
        "debug shares the captured generation's provider path plus the log \
         insert (observed {count})"
    );
    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_logs")
        .fetch_one(&pool)
        .await
        .expect("log rows readable");
    assert_eq!(
        logs, 1,
        "the debug route persists its log before responding"
    );
}
