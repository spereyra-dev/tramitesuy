//! Pool setup and the embedded-migrations runner (D-6). This module is the
//! only place where `migrations/` is loaded; repositories and providers live
//! in sibling modules from B2/B4 onward.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Connects a pool to the given database URL (dev story: the compose `db`
/// service, `postgres://postgres:postgres@localhost:5432/tramitesuy`).
pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(30))
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
