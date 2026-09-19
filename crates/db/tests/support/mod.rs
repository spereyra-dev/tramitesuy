//! Shared helpers for the S1 baseline-instrumentation tests (task 2): the
//! SQL-statement counter wrapped around a fresh scratch database.
//
// Each test binary compiles this module independently, so helpers consumed
// by only some of the binaries trip dead_code here; allowed module-wide
// (same pattern as the other crates' test support).
#![allow(dead_code)]

use sqlx::PgPool;

pub use db::test_support::sql_counter::SqlCounter;

/// Fresh scratch database (extensions + migrations applied) plus a pool
/// whose connections log every executed statement to the counter.
pub async fn fresh_migrated_counting_db() -> (PgPool, SqlCounter) {
    let (pool, name) = crate::common::create_test_db().await;
    crate::common::provision_extensions(&pool).await;
    crate::common::apply_migrations(&pool).await;
    drop(pool);

    let counter = SqlCounter::new();
    let base = std::env::var("TRAMITESUY_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    let url = format!("{}/{}", base.trim_end_matches("/postgres"), name);
    let pool = counter.counting_pool(&url).await.expect("counting pool");
    (pool, counter)
}
