//! Shared helpers for the B1 database integration tests.
//!
//! Tests run against the compose Postgres (service `db`, D-6). Each test
//! creates a uniquely named scratch database, provisions the extensions the
//! docker init SQL provides on a real dev instance (`pg_trgm`, `unaccent` —
//! migrations themselves must stay portable per design §6), applies the
//! embedded migrations, and drops the database at the end of the test.
//
// Each test binary compiles this module independently, so helpers consumed
// by only some of the binaries trip dead_code here; allowed module-wide
// (same pattern as crates/search tests/support).
#![allow(dead_code)]

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::types::Uuid;

/// Audited: names are generated internally (`b1_<pid>_<nanos>`), never from
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
    format!("b1_{}_{}", std::process::id(), nanos)
}

/// Creates a uniquely named scratch database and connects a pool to it.
pub async fn create_test_db() -> (PgPool, String) {
    let name = unique_db_name();
    let admin = admin_pool().await;
    sqlx::query(audited(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("create scratch test database");
    let url = format!("{}/{}", admin_url().trim_end_matches("/postgres"), name);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect to scratch test database");
    (pool, name)
}

/// Provisions the extensions the docker init SQL provides on a dev instance.
/// Migrations must NOT create extensions themselves (design §6, D-6), so tests
/// simulate a pre-provisioned instance exactly as the compose `db` service is.
pub async fn provision_extensions(pool: &PgPool) {
    for ext in ["pg_trgm", "unaccent"] {
        sqlx::query(audited(format!("CREATE EXTENSION IF NOT EXISTS {ext}")))
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("provision extension {ext}: {e:?}"));
    }
}

/// Applies the embedded migrations under test.
pub async fn apply_migrations(pool: &PgPool) {
    db::pool::run_migrations(pool)
        .await
        .expect("embedded migrations apply cleanly");
}

/// Scratch database with extensions provisioned, migrations NOT yet applied.
pub async fn fresh_provisioned_db() -> (PgPool, String) {
    let (pool, name) = create_test_db().await;
    provision_extensions(&pool).await;
    (pool, name)
}

/// Scratch database with extensions provisioned and migrations applied.
pub async fn fresh_migrated_db() -> (PgPool, String) {
    let (pool, name) = fresh_provisioned_db().await;
    apply_migrations(&pool).await;
    (pool, name)
}

/// Best-effort cleanup; called explicitly at the end of each test.
pub async fn drop_test_db(name: &str) {
    let admin = admin_pool().await;
    sqlx::query(audited(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .expect("drop scratch test database");
}

pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"))
}

pub fn is_fk_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23503"))
}

/// Minimal fixture rows covering every FK target, inserted directly via SQL.
pub struct Seed {
    pub category_id: Uuid,
    pub organization_id: Uuid,
    pub event_id: Uuid,
    pub procedure_id: Uuid,
    pub version_id: Uuid,
    pub log_id: Uuid,
}

pub async fn seed_minimal(pool: &PgPool) -> Seed {
    let category_id: Uuid = sqlx::query_scalar(
        "INSERT INTO categories (slug, name, order_index) VALUES ('vehiculos', 'Vehículos', 1) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed category");
    let organization_id: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (external_id, name) VALUES ('org-1', 'MTOP') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed organization");
    let event_id: Uuid = sqlx::query_scalar(
        "INSERT INTO life_events (slug, name, description, category_id) VALUES ('comprar-vehiculo', 'Comprar un vehículo', 'Trámites para comprar.', $1) RETURNING id",
    )
    .bind(category_id)
    .fetch_one(pool)
    .await
    .expect("seed life event");
    let procedure_id: Uuid = sqlx::query_scalar(
        "INSERT INTO procedures (external_id, name, organization_id, status) VALUES ('100001', 'Trámite 1', $1, 'active') RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(pool)
    .await
    .expect("seed procedure");
    sqlx::query(
        "INSERT INTO life_event_procedures (life_event_id, procedure_id, order_index, required) VALUES ($1, $2, 1, TRUE)",
    )
    .bind(event_id)
    .bind(procedure_id)
    .execute(pool)
    .await
    .expect("seed relation");
    let version_id: Uuid = sqlx::query_scalar(
        "INSERT INTO procedure_versions (procedure_id, content_hash, payload) VALUES ($1, 'hash-a', '{}'::jsonb) RETURNING id",
    )
    .bind(procedure_id)
    .fetch_one(pool)
    .await
    .expect("seed version");
    let log_id: Uuid = sqlx::query_scalar(
        "INSERT INTO search_logs (query, normalized_query) VALUES ('compre un auto', 'compre auto') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed log");
    Seed {
        category_id,
        organization_id,
        event_id,
        procedure_id,
        version_id,
        log_id,
    }
}
