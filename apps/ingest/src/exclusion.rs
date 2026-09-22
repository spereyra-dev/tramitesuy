//! The ingestion exclusion (task 35, S11, OPT-02, design §6.2): at most
//! one ingestion/publication execution is active per installation.
//! Scheduled and manual runs acquire the same PostgreSQL advisory lock
//! (`hashtext('tramitesuy:ingestion')`); a run that cannot acquire it
//! terminates with a recorded `skipped` status and is not queued, while
//! the API keeps serving the current generation throughout.
//!
//! The lock is TRANSACTION-scoped: it lives on the guard's transaction, so
//! it is released when the guard ends — commit, rollback, drop, or an
//! unwind (panic) — and no error or panic path can leave a stuck
//! exclusion.

use sqlx::PgPool;

/// The advisory-lock key shared by every ingestion/publication run of the
/// installation (scheduled, manual, recovery).
pub const EXCLUSION_HASHTEXT: &str = "tramitesuy:ingestion";

/// The held exclusion. Dropping it releases the lock (the guard's
/// transaction is rolled back), on every exit path including a panic.
pub struct IngestionExclusion {
    // Suppression justification: the transaction is held purely for its
    // lifetime — the lock it carries releases when the guard is dropped —
    // so the field is intentionally never read.
    #[expect(dead_code)]
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
}

impl IngestionExclusion {
    /// Attempts to acquire the ingestion exclusion. `Some(guard)` means
    /// this run holds it for the guard's lifetime; `None` means another
    /// run holds it (the caller terminates with a recorded `skipped`
    /// status and is not queued).
    pub async fn try_acquire(pool: &PgPool) -> Result<Option<IngestionExclusion>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let acquired: bool = sqlx::query_scalar!(
            "SELECT pg_try_advisory_xact_lock(hashtext('tramitesuy:ingestion')) \
             AS \"acquired!\"",
        )
        .fetch_one(&mut *tx)
        .await?;
        if acquired {
            Ok(Some(IngestionExclusion { tx }))
        } else {
            // Drop the open transaction: the lock was not acquired and the
            // run terminates (nothing is queued).
            Ok(None)
        }
    }
}
