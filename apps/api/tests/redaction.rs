//! Task 81 (API-10): privacy redaction before persistence — the Uruguayan
//! cédula, phone, and email patterns are replaced with `<REDACTED>`; the raw
//! document number appears nowhere in `search_logs`; redaction touches only
//! the log copy, while the response still echoes the query as sent.
//!
//! Task 82 (GREEN): `handlers/search.rs` + `crates/db::repos::search_log`
//! persist ONLY the redacted query, normalized query, selected/top event ids,
//! top score, and timestamp. The exact-column-set allowlist lives in
//! `crates/db/tests/search_log.rs` (task 83).

mod support;

use support::*;

/// Runs one search, then reads the single log row the request persisted.
async fn logged_row(
    app: &axum::Router,
    pool: &sqlx::PgPool,
    query_encoded: &str,
) -> (serde_json::Value, (String, String)) {
    let (status, body) = request(app, "GET", &format!("/api/v1/search?q={query_encoded}")).await;
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "search succeeds: {body}"
    );

    let row: (String, String) = sqlx::query_as("SELECT query, normalized_query FROM search_logs")
        .fetch_one(pool)
        .await
        .expect("the search persisted exactly one log row");
    (body, row)
}

#[tokio::test(flavor = "multi_thread")]
async fn cedula_query_is_redacted_in_the_log_but_not_in_the_response() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool.clone());

    let (body, (query, normalized)) =
        logged_row(&app, &pool, "perdi%20mi%20cedula%204.123.456-7").await;

    assert_eq!(
        body["query"], "perdi mi cedula 4.123.456-7",
        "the response echoes the query as sent (redaction is log-copy only)"
    );
    assert_eq!(
        query, "perdi mi cedula <REDACTED>",
        "the stored query is redacted"
    );
    let stored = format!("{query}|{normalized}");
    assert!(
        !stored.contains("4.123.456-7") && !stored.contains("41234567"),
        "the raw document number appears nowhere in search_logs: {stored}"
    );

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn phone_numbers_are_redacted_in_the_log() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool.clone());

    let (_, (query, _)) = logged_row(&app, &pool, "llamame%20al%20099%20123%20456").await;
    assert_eq!(query, "llamame al <REDACTED>", "domestic mobile redacted");

    common_drop(&db_name).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn email_addresses_are_redacted_in_the_log() {
    let (pool, db_name) = fresh_migrated_db().await;
    let app = spawn_app(pool.clone());

    let (_, (query, _)) = logged_row(
        &app,
        &pool,
        "escribi%20a%20juan.perez%40gmail.com%20gracias",
    )
    .await;
    assert_eq!(
        query, "escribi a <REDACTED> gracias",
        "email addresses are redacted"
    );

    common_drop(&db_name).await;
}

/// Open-mode fixture (same shape as search_modes.rs).
async fn seed_search_fixture(pool: &sqlx::PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1)",
    )
    .execute(pool)
    .await
    .expect("seed category");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'comprar-vehiculo', 'Adquisición de rodados', \
                'Pasos para adquirir un rodado en Uruguay.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event");
    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name, official_url) \
         VALUES ('org-1', 'Ministerio de Transporte', 'MTOP', 'https://www.gub.uy/mtop')",
    )
    .execute(pool)
    .await
    .expect("seed organization");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '4551', 'Solicitud de empadronamientos', 'Empadronamiento ante la DNT.', \
                o.id, 'https://www.gub.uy/tramite/4551', 'active', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb, \
                '2026-09-18T03:00:00Z', '2026-09-18T03:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '4551'",
    )
    .execute(pool)
    .await
    .expect("seed relation");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_search_persists_one_log_row_with_specced_fields_only() {
    let (pool, db_name) = fresh_migrated_db().await;
    seed_search_fixture(&pool).await;
    let app = spawn_app(pool.clone());

    let (status, _) = request(&app, "GET", "/api/v1/search?q=compre%20un%20auto%20usado").await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let row: (
        Option<sqlx::types::Uuid>,
        Option<sqlx::types::Uuid>,
        Option<f64>,
        Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT selected_event_id, top_event_id, top_score, created_at FROM search_logs",
    )
    .fetch_one(&pool)
    .await
    .expect("one log row persisted");
    let event_id: (sqlx::types::Uuid,) =
        sqlx::query_as("SELECT id FROM life_events WHERE slug = 'comprar-vehiculo'")
            .fetch_one(&pool)
            .await
            .expect("seeded event readable");
    assert_eq!(row.0, Some(event_id.0), "selected event id persisted");
    assert_eq!(row.1, Some(event_id.0), "top event id persisted");
    assert_eq!(row.2, Some(36.0), "top score persisted");
    assert!(row.3.is_some(), "the log row carries a timestamp");

    common_drop(&db_name).await;
}
