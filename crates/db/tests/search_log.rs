//! Tasks 82–83 (API-10, DM-1): the search-log repository persists only the
//! specced telemetry, and the `search_logs` schema equals the specced column
//! set EXACTLY — a future field cannot silently widen telemetry, which is
//! also the structural proof that no IP/user-agent/name/contact column exists.

#[path = "c2support/mod.rs"]
mod c2support;

use c2support::*;
use db::repos::search_log::{self, NewSearchLog};

/// The specced `search_logs` column set (data-model spec, DM-1 table 9).
const SPECCED_COLUMNS: [&str; 7] = [
    "created_at",
    "id",
    "normalized_query",
    "query",
    "selected_event_id",
    "top_event_id",
    "top_score",
];

#[tokio::test(flavor = "multi_thread")]
async fn search_logs_columns_equal_the_specced_set_exactly() {
    let (pool, db_name) = fresh_migrated_db().await;

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'search_logs' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("readable information_schema");

    let columns: Vec<&str> = rows.iter().map(|(name,)| name.as_str()).collect();
    assert_eq!(
        columns, SPECCED_COLUMNS,
        "search_logs must have exactly the specced columns — no IP, user agent, \
         name, or contact column, and no silent widening"
    );

    drop_db(&db_name).await;
}

/// Seeds one event so the slug→id FK resolution has a target.
async fn seed_one_event(pool: &sqlx::PgPool) -> sqlx::types::Uuid {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'alta-vehiculo', 'Alta de vehiculos', 'Registro inicial.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event");
    let (id,): (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'alta-vehiculo'")
            .fetch_one(pool)
            .await
            .expect("seeded event readable");
    id
}

#[tokio::test(flavor = "multi_thread")]
async fn search_log_insert_persists_only_the_specced_fields() {
    let (pool, db_name) = fresh_migrated_db().await;
    let event_id = seed_one_event(&pool).await;

    let log_id = search_log::insert(
        &pool,
        &NewSearchLog {
            query: "perdi mi cedula <REDACTED>".to_string(),
            normalized_query: "perdi cedula redacted".to_string(),
            selected_event_slug: Some("alta-vehiculo".to_string()),
            top_event_slug: Some("alta-vehiculo".to_string()),
            top_score: Some(36),
        },
    )
    .await
    .expect("log row inserts");

    let row: (
        String,
        String,
        Option<sqlx::types::Uuid>,
        Option<sqlx::types::Uuid>,
        Option<f64>,
        bool,
    ) = sqlx::query_as(
        "SELECT query, normalized_query, selected_event_id, top_event_id, top_score, \
                created_at IS NOT NULL \
         FROM search_logs WHERE id = $1",
    )
    .bind(log_id)
    .fetch_one(&pool)
    .await
    .expect("log row readable");

    assert_eq!(row.0, "perdi mi cedula <REDACTED>");
    assert_eq!(row.1, "perdi cedula redacted");
    assert_eq!(row.2, Some(event_id), "selected_event_id resolves the slug");
    assert_eq!(row.3, Some(event_id), "top_event_id resolves the slug");
    assert_eq!(row.4, Some(36.0));
    assert!(row.5, "created_at is stamped");

    drop_db(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_event_slugs_persist_as_null_event_ids() {
    let (pool, db_name) = fresh_migrated_db().await;

    let log_id = search_log::insert(
        &pool,
        &NewSearchLog {
            query: "compre un auto usado".to_string(),
            normalized_query: "compre auto usado".to_string(),
            selected_event_slug: Some("comprar-vehiculo".to_string()),
            top_event_slug: Some("comprar-vehiculo".to_string()),
            top_score: Some(36),
        },
    )
    .await
    .expect("log row inserts even when the event is absent from the projection");

    let row: (Option<sqlx::types::Uuid>, Option<sqlx::types::Uuid>) =
        sqlx::query_as("SELECT selected_event_id, top_event_id FROM search_logs WHERE id = $1")
            .bind(log_id)
            .fetch_one(&pool)
            .await
            .expect("log row readable");
    assert_eq!(
        row,
        (None, None),
        "an engine event absent from the DB projection stores NULL ids (nullable FKs)"
    );

    drop_db(&db_name).await;
}
