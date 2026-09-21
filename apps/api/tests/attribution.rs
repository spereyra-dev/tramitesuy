//! Task 74 (API-4): every procedure-bearing payload — event pages and
//! procedure detail — carries the complete attribution block:
//! `source.official = true`, `source.name` = the exact source catalog name,
//! `source.official_url`, `source.last_synced_at` equal to the last
//! ingestion run that touched the procedure, and `source.license =
//! "odc-uy"`. The shared helper in `support::assert_source_attribution` is
//! applied to EVERY procedure payload found in the response tree.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use support::*;

/// Collects every procedure-bearing object (has both `external_id` and
/// `source`) in a response payload.
fn collect_procedure_payloads<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("external_id") && map.contains_key("source") {
                out.push(value);
            }
            for child in map.values() {
                collect_procedure_payloads(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_procedure_payloads(item, out);
            }
        }
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_procedure_payload_on_an_event_page_carries_full_attribution() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    let (status, body) = request(&app, "GET", "/api/v1/events/comprar-vehiculo").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    let mut payloads = Vec::new();
    collect_procedure_payloads(&body, &mut payloads);
    assert_eq!(payloads.len(), 2, "both relations must carry attribution");

    for payload in payloads {
        let external_id = payload["external_id"].as_str().expect("external_id");
        let official_url = payload["official_url"].as_str();
        let expected = last_seen_of(&pool, external_id).await;
        assert_source_attribution(&payload["source"], official_url, &expected);
    }

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn procedure_detail_carries_full_attribution() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/2368").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_source_attribution(
        &body["source"],
        Some("https://www.gub.uy/tramite/2368"),
        &last_seen_of(&pool, "2368").await,
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deactivated_procedures_attribution_stays_intact() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool.clone()).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/6995").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_source_attribution(
        &body["source"],
        Some("https://www.gub.uy/tramite/6995"),
        &last_seen_of(&pool, "6995").await,
    );

    common_drop(&db_name).await;
}
