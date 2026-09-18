//! Task 71 (API-6, TX-6): `GET /events/:slug` returns the event's name,
//! description, category, and its procedures ordered by `order_index`, each
//! carrying `order`, `required`, `official_url`, and the attribution block;
//! an unknown slug returns 404. Attribution/cost assertions reuse the shared
//! helpers in `support` (tasks 74/75).

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn event_page_returns_ordered_procedures_with_attribution() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, body) = request(&app, "GET", "/api/v1/events/comprar-vehiculo").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_hyphen_slugs(&body);

    assert_eq!(body["slug"], "comprar-vehiculo");
    assert_eq!(body["name"], "Comprar un vehículo");
    assert_eq!(
        body["description"],
        "Requisitos y trámites para comprar un vehículo."
    );
    assert_eq!(body["category"], "vehiculos");

    let procedures = body["procedures"].as_array().expect("procedures array");
    assert_eq!(procedures.len(), 2, "body: {body}");

    // Ordered by order_index even though the fixture inserted order 2 first.
    let first = &procedures[0];
    assert_eq!(first["external_id"], "4551");
    assert_eq!(first["name"], "Solicitud de empadronamientos");
    assert_eq!(first["order"], 1);
    assert_eq!(first["required"], serde_json::json!(true));
    assert_eq!(first["official_url"], "https://www.gub.uy/tramite/4551");
    assert_source_attribution(
        &first["source"],
        Some("https://www.gub.uy/tramite/4551"),
        &last_seen_of(&pool, "4551").await,
    );

    let second = &procedures[1];
    assert_eq!(second["external_id"], "2368");
    assert_eq!(second["order"], 2);
    assert_eq!(second["required"], serde_json::json!(false));
    assert_source_attribution(
        &second["source"],
        Some("https://www.gub.uy/tramite/2368"),
        &last_seen_of(&pool, "2368").await,
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn event_page_keeps_a_deactivated_related_procedure_visible() {
    // Relations are never deleted (IN-7): a deactivated procedure stays on
    // its event page with its attribution block intact.
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, body) = request(&app, "GET", "/api/v1/events/vender-vehiculo").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let procedures = body["procedures"].as_array().expect("procedures array");
    assert_eq!(procedures.len(), 2);
    assert_eq!(procedures[0]["external_id"], "6995");
    assert_eq!(procedures[0]["order"], 1);
    assert_source_attribution(
        &procedures[0]["source"],
        Some("https://www.gub.uy/tramite/6995"),
        &last_seen_of(&pool, "6995").await,
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_event_slug_returns_404() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, _) = request(&app, "GET", "/api/v1/events/no-existe").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    common_drop(&db_name).await;
}
