//! Task 75 (API-3): the missing-cost rule — empty `tiene_costo`/`valor`
//! (or absent raw data) yields `cost: null` with the exact
//! `cost_display: "Sin costo informado"`; a populated source value passes
//! through verbatim; no code path defaults or estimates a cost.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn empty_cost_renders_null_with_sin_costo_informado() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        body["cost"],
        Value::Null,
        "an empty tiene_costo/valor must render cost as null"
    );
    assert_eq!(body["cost_display"], "Sin costo informado");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn populated_cost_passes_through_verbatim() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/2368").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(
        body["cost"], "55.70",
        "the source value must pass through verbatim"
    );
    assert_eq!(body["cost_display"], "55.70");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn absent_raw_data_never_defaults_or_estimates_a_cost() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    let (status, body) = request(&app, "GET", "/api/v1/procedures/7001").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["cost"], Value::Null);
    assert_eq!(body["cost_display"], "Sin costo informado");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn event_page_procedure_cards_share_the_same_wording() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app_with_generation(pool).await;

    let (status, body) = request(&app, "GET", "/api/v1/events/comprar-vehiculo").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let procedures = body["procedures"].as_array().expect("procedures array");

    assert_eq!(procedures[0]["external_id"], "4551");
    assert_eq!(procedures[0]["cost"], Value::Null);
    assert_eq!(procedures[0]["cost_display"], "Sin costo informado");

    assert_eq!(procedures[1]["external_id"], "2368");
    assert_eq!(procedures[1]["cost"], "55.70");
    assert_eq!(procedures[1]["cost_display"], "55.70");

    common_drop(&db_name).await;
}
