//! Shared scratch-DB helpers for the B5 ingest integration tests.
//!
//! Tests run against the compose Postgres (service `db`, D-6). Each test
//! creates a uniquely named scratch database (`b5_<pid>_<nanos>`),
//! provisions the extensions the docker init SQL provides on a real dev
//! instance, applies the embedded migrations, and drops the database at the
//! end with `DROP DATABASE ... WITH (FORCE)`.
//
// Each test binary compiles this module independently, so helpers consumed
// by only some of the binaries trip dead_code here; allowed module-wide
// (same pattern as crates/search tests/support and crates/db tests/common).
#![allow(dead_code)]

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::path::Path;

/// Audited: names are generated internally (`b5_<pid>_<nanos>`), never from
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
    format!("b5_{}_{}", std::process::id(), nanos)
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

/// Provisions the extensions the docker init SQL provides on a dev instance
/// (migrations must not create extensions themselves, design §6, D-6).
pub async fn provision_extensions(pool: &PgPool) {
    for ext in ["pg_trgm", "unaccent"] {
        sqlx::query(audited(format!("CREATE EXTENSION IF NOT EXISTS {ext}")))
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("provision extension {ext}: {e:?}"));
    }
}

/// Scratch database with extensions provisioned and migrations applied.
pub async fn fresh_migrated_db() -> (PgPool, String) {
    let (pool, name) = create_test_db().await;
    provision_extensions(&pool).await;
    db::pool::run_migrations(&pool)
        .await
        .expect("embedded migrations apply cleanly");
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

/// Inserts organization + procedure rows for every external id in the
/// committed snapshot file (relations' FK targets for the seed tests).
pub async fn seed_procedures_from_snapshot(pool: &PgPool, snapshot: &Path) {
    let text = std::fs::read_to_string(snapshot).expect("snapshot readable");
    let ids: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    seed_procedures(pool, &ids).await;
}

/// Inserts organization + procedure rows for the given external ids via
/// plain SQL (the export/seed tests only need FK targets to exist).
pub async fn seed_procedures(pool: &PgPool, external_ids: &[&str]) {
    let org_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO organizations (external_id, name) VALUES ('org-export', 'AGESIC') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("seed organization");
    for id in external_ids {
        sqlx::query(
            "INSERT INTO procedures (external_id, name, organization_id, status) \
             VALUES ($1, 'Trámite', $2, 'active')",
        )
        .bind(id)
        .bind(org_id)
        .execute(pool)
        .await
        .expect("seed procedure");
    }
}
