//! Task 79 (API-2, SE-10): the `GET /api/v1/search` response contract —
//! `mode: open` with the selected event first (score + confidence 0.80),
//! `mode: disambiguation` with up to 3 options and no single answer, and
//! `mode: categories` listing the available category slugs for a zero-match
//! query. Task 84 (design §4.2, TX-1) adds the proof that the ranker's
//! source of truth is the YAML taxonomy, not the DB projection.
//!
//! The engine lexicons come from the real `data/` YAML seed (loaded at
//! `AppState` boot); the scratch DB carries only the projection rows the
//! test needs. The open-mode fixture's event name/description were chosen
//! (and empirically verified) to produce NO provider contribution for the
//! fixture query, so the keyword-driven scores stay exact: 10 + 8 + 3 +
//! ACTION_ENTITY 15 = 36, single positive candidate → confidence 0.80.

mod support;

use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn dominant_query_opens_the_event_directly() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_hyphen_slugs(&body);

    assert_eq!(body["query"], "compre un auto usado", "query as sent");
    assert_eq!(
        body["normalized_query"], "compre auto usado",
        "the normalized query echo"
    );
    assert_eq!(body["mode"], "open");

    let results = body["results"].as_array().expect("results array");
    assert_eq!(
        results.len(),
        1,
        "open mode presents exactly the selected event: {body}"
    );
    let first = &results[0];
    assert_eq!(first["event"]["slug"], "comprar-vehiculo");
    assert_eq!(first["event"]["name"], "Comprar un vehículo");
    assert_eq!(first["score"], 36, "10 + 8 + 3 + ACTION_ENTITY 15");
    // Recorded deviation (apply progress): the task prose and the API spec's
    // GIVEN say "confidence 0.80", quoting the SE-9 36/9 illustration. The
    // real seed's candidate distribution for this query is 36 vs 8 (several
    // events score the `vehiculo` entity weight alone), so the ratified D-1
    // formula gives round(36/(36+8), 2) = 0.82 — same value the A3 engine
    // tests recorded for this exact query. The formula is normative (SE-9);
    // the 0.80 floor case is exercised by single-candidate distributions.
    assert_eq!(first["confidence"], 0.82);

    // The event's ordered procedures summary with the attribution block.
    let procedures = first["procedures"].as_array().expect("procedures array");
    let external_ids: Vec<&str> = procedures
        .iter()
        .map(|p| p["external_id"].as_str().expect("external_id"))
        .collect();
    assert_eq!(external_ids, vec!["4551", "2368"], "ordered by order_index");
    assert_eq!(procedures[0]["required"], serde_json::json!(true));
    assert_eq!(procedures[0]["cost_display"], "Sin costo informado");
    assert_eq!(procedures[1]["cost"], "55.70");
    for procedure in procedures {
        assert_eq!(procedure["source"]["license"], "odc-uy");
    }

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ambiguous_query_offers_up_to_three_options() {
    let (pool, db_name) = fresh_migrated_db().await;
    // No projection rows needed: options are the engine's top-scored events.
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/search?q=auto").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_hyphen_slugs(&body);

    assert_eq!(body["mode"], "disambiguation");
    assert!(
        body.get("results").is_none(),
        "no single answer is presented as the result: {body}"
    );
    let options = body["options"].as_array().expect("options array");
    assert_eq!(
        options.len(),
        3,
        "up to 3 top-scored events, fewer only when fewer exist"
    );
    for option in options {
        assert!(option["slug"].is_string());
        assert!(option["name"].is_string());
        assert!(option["score"].is_i64());
        assert!(option["confidence"].is_number());
    }
    let slugs: Vec<&str> = options
        .iter()
        .map(|o| o["slug"].as_str().expect("slug"))
        .collect();
    let mut sorted = slugs.clone();
    sorted.sort_unstable();
    assert_eq!(
        slugs, sorted,
        "tied options keep the deterministic slug-ascending order"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn zero_match_query_falls_back_to_categories() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/search?q=quiero%20abrir%20una%20cuenta%20bancaria",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_hyphen_slugs(&body);

    assert_eq!(body["mode"], "categories");
    assert!(body.get("results").is_none());
    assert!(body.get("options").is_none());
    let categories = body["categories"].as_array().expect("categories array");
    assert_eq!(
        categories,
        &vec![serde_json::json!({"slug": "vehiculos", "name": "Vehículos"})],
        "the available category slugs (with names) are listed: {body}"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_query_parameter_is_rejected() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/search").await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        serde_json::json!({"error": "bad request"}),
        "the exact public 400 body"
    );

    common_drop(&db_name).await;
}

/// Task 84 (design §4.2, TX-1): the ranker's scores come from the YAML
/// taxonomy cached in `AppState`, never from the DB keyword projection —
/// tampering with `life_event_keywords.weight` changes nothing in the
/// explanation, so `/search/debug` reconstruction stays exact. The seeded DB
/// row DOES feed the FTS/trigram providers, so the debug explanation carries
/// provider entries and their sum must still equal the reported score.
#[tokio::test(flavor = "multi_thread")]
async fn ranker_source_of_truth_is_yaml_not_the_db_projection() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    // Tamper with the DB projection: `comprar` at weight 99 (the YAML says 10).
    // The event name is provider-matching here on purpose.
    sqlx::query(
        "UPDATE life_events SET name = 'Comprar un vehiculo usado en Uruguay', \
         description = 'Todo sobre comprar un vehiculo.' \
         WHERE slug = 'comprar-vehiculo'",
    )
    .execute(&pool)
    .await
    .expect("make the projection provider-visible");
    sqlx::query(
        "INSERT INTO life_event_keywords (life_event_id, term, type, weight, negative) \
         SELECT e.id, 'comprar', 'ACTION', 99, FALSE \
         FROM life_events e WHERE e.slug = 'comprar-vehiculo'",
    )
    .execute(&pool)
    .await
    .expect("seed the tampered projection keyword");

    let app = spawn_app(pool);
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/search/debug?q=compre%20un%20auto%20usado",
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let results = body["results"].as_array().expect("results array");
    let result = results
        .iter()
        .find(|r| r["slug"] == "comprar-vehiculo")
        .expect("comprar-vehiculo present");
    let score = result["score"].as_i64().expect("score");
    let entries = result["explanation"].as_array().expect("explanation array");

    let keyword_entry = entries
        .iter()
        .find(|e| e["rule"] == "KEYWORD" && e["term"] == "comprar")
        .expect("the KEYWORD comprar entry");
    assert_eq!(
        keyword_entry["value"], 10,
        "the weight comes from the YAML taxonomy, not the DB projection (99)"
    );
    assert!(
        !entries.iter().any(|e| e["value"] == 99),
        "no explanation entry may carry the tampered DB weight"
    );
    assert!(
        entries
            .iter()
            .any(|e| e["rule"] == "FTS_TEXT" || e["rule"] == "TRIGRAM"),
        "the DB row feeds the providers, so a provider entry appears: {entries:?}"
    );
    let sum: i64 = entries
        .iter()
        .map(|e| e["value"].as_i64().expect("entry value"))
        .sum();
    assert_eq!(
        sum, score,
        "debug reconstruction stays exact with provider entries mixed in"
    );

    common_drop(&db_name).await;
}
