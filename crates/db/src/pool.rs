//! Pool setup and the embedded-migrations runner (D-6). This module is the
//! only place where `migrations/` is loaded; repositories and providers live
//! in sibling modules from B2/B4 onward.

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Default pool size: the previous hardcoded value (5 connections) is
/// preserved as the default (design §7.1).
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// Default connection acquire timeout: design §7.1 sets 500 ms, replacing
/// the previous hardcoded 30 s wait.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);

/// Connects a pool to the given database URL with caller-controlled sizing
/// and acquire timeout (design §7.1: no hardcoded pool limits). The dev
/// story default is the compose `db` service,
/// `postgres://postgres:postgres@localhost:5432/tramitesuy`.
pub async fn connect(
    url: &str,
    max_connections: u32,
    acquire_timeout: Duration,
) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(acquire_timeout)
        .connect(url)
        .await
}

/// Applies the embedded migrations from `../../migrations` (sqlx::migrate!).
/// Idempotent: already-applied versions are skipped.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map(|_| ())
}
