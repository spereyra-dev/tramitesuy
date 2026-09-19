//! Shared helpers for the C1 API integration tests.
//!
//! Tests run against the compose Postgres (service `db`, D-6). Each test
//! creates a uniquely named scratch database (`c1_<pid>_<nanos>`),
//! provisions the extensions the docker init SQL provides on a real dev
//! instance (`pg_trgm`, `unaccent`), applies the embedded migrations, seeds
//! the read-surface fixture, and drops the database at the end.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use api::dto;

/// The repository root (apps/api → repo root), where the real `data/` seed
/// lives; the search tests boot `AppState` from it exactly like production.
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Audited: names are generated internally (`c1_<pid>_<nanos>`), never from
/// user input; sqlx 0.9 requires an explicit safety assertion for dynamic SQL.
fn audited(sql: String) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(sql)
}

fn admin_url() -> String {
    std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string())
}

async fn admin_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url())
        .await
        .expect("connect to the compose Postgres admin database")
}

fn unique_db_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("c1_{}_{}", std::process::id(), nanos)
}

/// Creates a uniquely named scratch database, provisions the compose-init
/// extensions, and applies the embedded migrations. The name is retried on
/// a collision: parallel test binaries share the process id space and the
/// wall clock, so two `pid+nanos` draws can theoretically coincide
/// (observed once under full-workspace parallelism).
pub async fn fresh_migrated_db() -> (PgPool, String) {
    let admin = admin_pool().await;
    let mut name = unique_db_name();
    loop {
        let result = sqlx::query(audited(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await;
        match result {
            Ok(_) => break,
            // SQLSTATE 23505 on pg_database.datname = a concurrent parallel
            // test binary drew the same pid+nanos name; redraw and retry.
            Err(err)
                if matches!(
                    &err,
                    sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")
                ) =>
            {
                name = unique_db_name();
            }
            Err(err) => panic!("create scratch test database: {err:?}"),
        }
    }
    let url = format!("{}/{}", admin_url().trim_end_matches("/postgres"), name);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect to scratch test database");
    for ext in ["pg_trgm", "unaccent"] {
        sqlx::query(audited(format!("CREATE EXTENSION IF NOT EXISTS {ext}")))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("provision extension {ext}: {e:?}"));
    }
    db::pool::run_migrations(&pool)
        .await
        .expect("embedded migrations apply cleanly");
    (pool, name)
}

/// Builds the API router over the given pool (the in-process test server).
/// Boot loads the YAML taxonomy from the repository's real `data/` seed —
/// the same ranker source of truth as production (task 84).
pub fn spawn_app(pool: PgPool) -> Router {
    let state = api::state::AppState::build(pool, &repo_root().join("data"))
        .expect("boot AppState from the real data seed");
    api::build_router(state)
}

/// Boots the router with an injected metrics sink (task 1 seam): same boot
/// path as `spawn_app`, but the counters surface becomes test-readable.
pub fn spawn_app_with_metrics(
    pool: PgPool,
    metrics: std::sync::Arc<dyn api::metrics::Metrics>,
) -> Router {
    let state = api::state::AppState::build_with_metrics(pool, &repo_root().join("data"), metrics)
        .expect("boot AppState from the real data seed");
    api::build_router(state)
}

/// Sends one request to the in-process router and returns the status plus
/// the parsed JSON body (Null for empty bodies).
pub async fn request(app: &Router, method: &str, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body readable");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Sends one request carrying a JSON body (content-type application/json)
/// and returns the status plus the parsed JSON body.
pub async fn request_json(
    app: &Router,
    method: &str,
    uri: &str,
    json: &Value,
) -> (StatusCode, Value) {
    request_raw(app, method, uri, json.to_string().as_bytes()).await
}

/// Sends one request carrying a raw byte body (no content type) and returns
/// the status plus the parsed JSON body (Null for unparsable bodies).
pub async fn request_raw(
    app: &Router,
    method: &str,
    uri: &str,
    body_bytes: &[u8],
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body_bytes.to_vec()))
                .expect("well-formed request"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body readable");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Seeds the read-surface fixture: two categories (vehiculos first), two
/// events, one organization, four procedures (populated cost, missing cost,
/// deactivated, no raw data), and relations whose rows are inserted out of
/// order to prove `order_index` ordering.
pub async fn seed_read_fixture(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO categories (slug, name, icon, order_index) \
         VALUES ('vehiculos', 'Vehículos', 'car', 1), ('trabajo', 'Trabajo', 'work', 2)",
    )
    .execute(pool)
    .await
    .expect("seed categories");

    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'comprar-vehiculo', 'Comprar un vehículo', \
                'Requisitos y trámites para comprar un vehículo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event comprar-vehiculo");
    sqlx::query(
        "INSERT INTO life_events (slug, name, description, category_id) \
         SELECT 'vender-vehiculo', 'Vender un vehículo', \
                'Trámites para dar de baja o vender un vehículo.', id \
         FROM categories WHERE slug = 'vehiculos'",
    )
    .execute(pool)
    .await
    .expect("seed event vender-vehiculo");

    sqlx::query(
        "INSERT INTO organizations (external_id, name, short_name, official_url) \
         VALUES ('org-1', 'Ministerio de Transporte', 'MTOP', 'https://www.gub.uy/mtop')",
    )
    .execute(pool)
    .await
    .expect("seed organization");

    // Procedures: 4551 (empty cost), 2368 (populated cost), 6995
    // (deactivated), 7001 (NULL raw_data — the never-default-a-cost case).
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
    .expect("seed procedure 4551");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '2368', 'Alta de vehículos ante la DNT', 'Alta inicial del vehículo.', \
                o.id, 'https://www.gub.uy/tramite/2368', 'active', \
                '{\"tiene_costo\": \"1\", \"valor\": \"55.70\"}'::jsonb, \
                '2026-09-18T04:00:00Z', '2026-09-18T04:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 2368");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at, deactivated_at) \
         SELECT '6995', 'Registro de Automotoras', 'Registro de automotoras.', \
                o.id, 'https://www.gub.uy/tramite/6995', 'inactive', \
                '{\"tiene_costo\": \"\", \"valor\": \"\"}'::jsonb, \
                '2026-09-18T02:00:00Z', '2026-09-18T02:00:00Z', '2026-09-18T05:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 6995");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '7001', 'Trámite sin datos crudos', 'Sin raw_data.', \
                o.id, NULL, 'active', NULL, \
                '2026-09-18T01:00:00Z', '2026-09-18T01:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 7001");

    // Relations inserted out of order: the API must order by order_index.
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2368'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '4551'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'vender-vehiculo' AND p.external_id = '6995'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'vender-vehiculo' AND p.external_id = '7001'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
}

/// Drops the scratch database (explicit cleanup at the end of each test).
pub async fn common_drop(name: &str) {
    let admin = admin_pool().await;
    sqlx::query(audited(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .expect("drop scratch test database");
}

/// Task 74 (API-4): asserts the complete attribution block of one
/// procedure-bearing payload — `official: true`, the exact source catalog
/// name, the procedure's official URL, `last_synced_at` equal to the last
/// ingestion run that touched the procedure, and the `odc-uy` license.
pub fn assert_source_attribution(
    source: &Value,
    expected_official_url: Option<&str>,
    expected_last_synced_at: &str,
) {
    assert_eq!(
        source["official"],
        serde_json::json!(true),
        "source.official must be true"
    );
    assert_eq!(
        source["name"], "Catálogo de trámites y servicios del Estado — AGESIC",
        "source.name must be the exact source catalog name"
    );
    match expected_official_url {
        Some(url) => assert_eq!(
            source["official_url"], url,
            "source.official_url must carry the procedure's official URL"
        ),
        None => assert!(
            source["official_url"].is_null(),
            "source.official_url must be null when the procedure has none"
        ),
    }
    assert_eq!(
        source["last_synced_at"], expected_last_synced_at,
        "source.last_synced_at must equal the last run that touched the procedure"
    );
    assert_eq!(source["license"], "odc-uy", "source.license must be odc-uy");
}

/// Task 76 (TX-4): validates every event/category slug in an `/api/v1`
/// response payload against the hyphen convention. Applied to every payload
/// the read-endpoint tests parse.
pub fn assert_hyphen_slugs(payload: &Value) {
    dto::validate_slugs_in_response(payload)
        .unwrap_or_else(|err| panic!("response exposes an invalid slug: {err}"));
}

/// Reads a procedure's `last_seen_at` from the scratch database and formats
/// it exactly as the API serializes `source.last_synced_at`.
pub async fn last_seen_of(pool: &PgPool, external_id: &str) -> String {
    let row: (sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>,) =
        sqlx::query_as("SELECT last_seen_at FROM procedures WHERE external_id = $1")
            .bind(external_id)
            .fetch_one(pool)
            .await
            .expect("procedure row readable");
    row.0.to_rfc3339()
}

pub use db::test_support::sql_counter::SqlCounter;

/// Fresh migrated scratch database plus a statement-counting pool over it
/// (task 2 instrument reused from `crates/db`'s test-support feature).
pub async fn fresh_migrated_counting_db() -> (PgPool, SqlCounter) {
    let (pool, name) = fresh_migrated_db().await;
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    let url = format!("{}/{}", base.trim_end_matches("/postgres"), name);
    drop(pool);

    let counter = SqlCounter::new();
    let pool = counter
        .counting_pool(&url)
        .await
        .expect("counting pool over the migrated scratch database");
    (pool, counter)
}
/// Seeds the projection rows for the open-mode contract: one category, one
/// event (name/description deliberately disjoint from the fixture query's
/// lexemes so FTS/trigram stay silent), one organization, two procedures
/// (empty cost and populated cost), and relations in declared order.
pub async fn seed_search_fixture(pool: &sqlx::PgPool) {
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
    .expect("seed procedure 4551");
    sqlx::query(
        "INSERT INTO procedures (external_id, name, description, organization_id, official_url, \
         status, raw_data, first_seen_at, last_seen_at) \
         SELECT '2368', 'Alta de vehículos ante la DNT', 'Alta inicial del vehículo.', \
                o.id, 'https://www.gub.uy/tramite/2368', 'active', \
                '{\"tiene_costo\": \"1\", \"valor\": \"55.70\"}'::jsonb, \
                '2026-09-18T04:00:00Z', '2026-09-18T04:00:00Z' \
         FROM organizations o WHERE o.external_id = 'org-1'",
    )
    .execute(pool)
    .await
    .expect("seed procedure 2368");

    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 1, TRUE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '4551'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 1");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) \
         SELECT e.id, p.id, 2, FALSE FROM life_events e, procedures p \
         WHERE e.slug = 'comprar-vehiculo' AND p.external_id = '2368'",
    )
    .execute(pool)
    .await
    .expect("seed relation order 2");
}
