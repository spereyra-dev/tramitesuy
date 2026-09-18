//! Task 72 (API-7): `GET /categories` lists slug, name, and `order_index`
//! ordered ascending (vehiculos first); `GET /categories/:slug/events`
//! lists that category's events with slug and name; an unknown category
//! slug returns 404.

mod support;

use axum::http::StatusCode;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn categories_list_is_order_index_ascending_starting_with_vehiculos() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/categories").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_hyphen_slugs(&body);

    let categories = body["categories"].as_array().expect("categories array");
    assert_eq!(categories.len(), 2);
    assert_eq!(categories[0]["slug"], "vehiculos");
    assert_eq!(categories[0]["name"], "Vehículos");
    assert_eq!(categories[0]["order_index"], 1);
    assert_eq!(categories[1]["slug"], "trabajo");
    assert_eq!(categories[1]["order_index"], 2);

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn category_events_listing_returns_slug_and_name() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, body) = request(&app, "GET", "/api/v1/categories/vehiculos/events").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_hyphen_slugs(&body);

    let events = body["events"].as_array().expect("events array");
    assert_eq!(events.len(), 2);
    let slugs: Vec<&str> = events
        .iter()
        .map(|e| e["slug"].as_str().expect("slug string"))
        .collect();
    assert_eq!(slugs, vec!["comprar-vehiculo", "vender-vehiculo"]);
    assert_eq!(events[0]["name"], "Comprar un vehículo");
    assert_eq!(events[1]["name"], "Vender un vehículo");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_category_slug_returns_404() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_read_fixture(&pool).await;
    let app = spawn_app(pool);

    let (status, _) = request(&app, "GET", "/api/v1/categories/no-existe/events").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    common_drop(&db_name).await;
}
