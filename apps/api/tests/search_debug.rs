//! Task 80 (API-5, SE-11, SE-3): the `GET /search/debug` contract — tokens
//! carrying `original` and `canonical` (showing `coche → vehiculo`), and per
//! result an `explanation` array whose entries carry `rule`, `term`/
//! `canonical` where applicable, and a value; the entries sum EXACTLY to the
//! reported score, making every ranking hand-reconstructible.
//!
//! No DB projection rows are seeded, so no FTS_TEXT/TRIGRAM entries appear
//! here (provider entries + reconstruction are proven in
//! `search_modes::ranker_source_of_truth_is_yaml_not_the_db_projection`).

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn debug_exposes_tokens_with_original_and_canonical_forms() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/search/debug?q=compre%20un%20coche").await;
    assert_eq!(status, axum::http::StatusCode::OK);

    assert_eq!(body["query"], "compre un coche", "query as sent");
    assert_eq!(body["normalized_query"], "compre coche");
    let tokens = body["tokens"].as_array().expect("tokens array");
    assert_eq!(
        tokens,
        &vec![
            serde_json::json!({"original": "compre", "canonical": "compre"}),
            serde_json::json!({"original": "coche", "canonical": "vehiculo"}),
        ],
        "the synonym resolution coche → vehiculo must be visible: {body}"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn explanation_entries_sum_exactly_to_the_reported_score() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/search/debug?q=compre%20un%20coche").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_hyphen_slugs(&body);

    let results = body["results"].as_array().expect("results array");
    let first = &results[0];
    assert_eq!(first["slug"], "comprar-vehiculo");
    assert_eq!(first["confidence"], 0.80);
    let score = first["score"].as_i64().expect("score");
    let entries = first["explanation"].as_array().expect("explanation array");

    for entry in entries {
        assert!(entry["rule"].is_string(), "every entry names its rule");
        assert!(entry["value"].is_i64(), "every entry carries its value");
    }
    let sum: i64 = entries
        .iter()
        .map(|e| e["value"].as_i64().expect("entry value"))
        .sum();
    assert_eq!(
        sum, score,
        "SE-11: the explanation entries reconstruct the score exactly"
    );

    // The hand-reconstructible case: 10 + 8 + ACTION_ENTITY 15 = 33.
    let keyword_entries: Vec<&serde_json::Value> =
        entries.iter().filter(|e| e["rule"] == "KEYWORD").collect();
    assert!(
        keyword_entries
            .iter()
            .any(|e| e["term"] == "comprar" && e["value"] == 10),
        "KEYWORD comprar +10: {keyword_entries:?}"
    );
    assert!(
        keyword_entries
            .iter()
            .any(|e| e["canonical"] == "vehiculo" && e["value"] == 8),
        "the synonym match is attributed to canonical vehiculo at its weight: {keyword_entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|e| e["rule"] == "ACTION_ENTITY" && e["value"] == 15),
        "the ACTION_ENTITY bonus is reported: {entries:?}"
    );

    // The reconstruction property holds for EVERY result, not just top1.
    for result in results {
        let entries = result["explanation"].as_array().expect("explanation array");
        let sum: i64 = entries
            .iter()
            .map(|e| e["value"].as_i64().expect("entry value"))
            .sum();
        assert_eq!(
            sum,
            result["score"].as_i64().expect("score"),
            "result {} reconstructs exactly",
            result["slug"]
        );
    }

    common_drop(&db_name).await;
}
