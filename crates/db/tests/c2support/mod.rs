//! Shared helpers for the C2 database integration tests (providers and the
//! search-log repo). Same scratch-DB lifecycle as B1's `common` module, with
//! a `c2_` name prefix and the collision retry C1 added after observing two
//! parallel test binaries draw the same `pid+nanos` name on Windows.
//
// Each test binary compiles this module independently, so helpers consumed
// by only some of the binaries trip dead_code here; allowed module-wide
// (same pattern as the other crates' test support).
#![allow(dead_code)]

use sqlx::postgres::{PgPool, PgPoolOptions};

/// Audited: names are generated internally (`c2_<pid>_<nanos>`), never from
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
    format!("c2_{}_{}", std::process::id(), nanos)
}

/// Creates a uniquely named scratch database (`c2_` prefix), retries on the
/// SQLSTATE 23505 name collision, provisions the compose-init extensions
/// (`pg_trgm`, `unaccent`), and applies the embedded migrations.
pub async fn fresh_migrated_db() -> (PgPool, String) {
    let admin = admin_pool().await;
    let mut name = unique_db_name();
    loop {
        let result = sqlx::query(audited(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await;
        match result {
            Ok(_) => break,
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

/// Drops the scratch database (explicit cleanup at the end of each test).
pub async fn drop_db(name: &str) {
    let admin = admin_pool().await;
    sqlx::query(audited(format!(
        "DROP DATABASE IF EXISTS {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .expect("drop scratch test database");
}

/// Fresh scratch database plus a pool whose connections log every executed
/// statement to the shared SQL counter (task 6): the same `c2_` scratch
/// lifecycle, reconnected through `SqlCounter::counting_pool` after the
/// migrations. The measurement section spans the counting-pool setup (same
/// discipline as `apps/api/tests/support::fresh_counting_db_section`), so
/// connection-establishment statements land inside the held section and the
/// test's `reset()` clears them before the measured work.
pub async fn fresh_migrated_counting_db() -> (
    PgPool,
    String,
    db::test_support::sql_counter::SqlCounter,
    db::test_support::sql_counter::SqlSection,
) {
    let (pool, name) = fresh_migrated_db().await;
    pool.close().await;
    let counter = db::test_support::sql_counter::SqlCounter::new();
    let section = counter.section().await;
    let url = format!("{}/{}", admin_url().trim_end_matches("/postgres"), name);
    let counting = counter
        .counting_pool(&url)
        .await
        .expect("counting pool connects to the same scratch database");
    (counting, name, counter, section)
}
