//! Task 73 (API-8): `GET /procedures/:id` returns name, description,
//! organization, official_url, cost fields per the missing-cost rule,
//! status, and the attribution block; a deactivated procedure remains
//! fetchable with `status: "inactive"`.

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn procedure_detail_carries_the_full_contract() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, body) = request(&app, "GET", "/api/v1/procedures/4551").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_hyphen_slugs(&body);

    assert_eq!(body["external_id"], "4551");
    assert_eq!(body["name"], "Solicitud de empadronamientos");
    assert_eq!(body["description"], "Empadronamiento ante la DNT.");
    assert_eq!(body["organization"], "Ministerio de Transporte");
    assert_eq!(body["official_url"], "https://www.gub.uy/tramite/4551");
    assert_eq!(body["status"], "active");
    assert_source_attribution(
        &body["source"],
        Some("https://www.gub.uy/tramite/4551"),
        &last_seen_of(&pool, "4551").await,
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deactivated_procedure_still_returns_200_with_status_inactive() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, body) = request(&app, "GET", "/api/v1/procedures/6995").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a deactivated procedure must remain fetchable; body: {body}"
    );
    assert_eq!(body["status"], "inactive");
    assert_source_attribution(
        &body["source"],
        Some("https://www.gub.uy/tramite/6995"),
        &last_seen_of(&pool, "6995").await,
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_procedure_id_returns_404() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, _) = request(&app, "GET", "/api/v1/procedures/no-existe").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    common_drop(&db_name).await;
}
