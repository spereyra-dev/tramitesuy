//! Retention and gated collection (S8 task 24, design §2.3): the worker-side
//! garbage collector for generations beyond the retention window. Collection
//! is off the request path, gated on the confirmed adoption of the newest
//! publication AND the in-flight drain (the API's captured `Arc` released,
//! reported through the adoption record) or the retention window passing, is
//! idempotent, and never touches the active or previous generation. The
//! request path never issues collection work.

use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

use super::adopt::AdoptionState;

use std::time::Duration;

/// Collection policy: how many generations to retain (default 3: active +
/// previous recoverable + one margin) and how long a beyond-retention
/// generation may keep waiting for its in-flight holder (the retention
/// window). Both are worker-side configuration parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionConfig {
    pub retention: usize,
    pub retention_window: Duration,
}

/// What one collection pass did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollectReport {
    /// Generations whose projections were deleted (manifests retired).
    pub collected: Vec<Uuid>,
    /// Generations deferred because an in-flight holder still uses them
    /// while the retention window is open.
    pub deferred_in_flight: Vec<Uuid>,
    /// Whether the pass found the newest publication NOT confirmed adopted
    /// (a lagging API) and therefore deleted nothing at all.
    pub deferred_unadopted: bool,
    pub deleted_rows: u64,
}

/// Deletes the projections of generations beyond retention, gated as the
/// catalog-generations delta requires:
///
/// 1. Only after the newest published generation is confirmed adopted by the
///    API's manifest write-back. A lagging API (or no adoption at all)
///    defers the whole pass — a lagging API's projection is never deleted.
/// 2. Per generation: only when no in-flight holder is reported, or the
///    retention window has passed since the adoption.
/// 3. Never the active or the previous generation: only rows beyond the
///    newest `retention` manifest rows are candidates, and an
///    already-retired generation is skipped (idempotency). Manifest rows are
///    retained as data; only `retired_at` is stamped.
pub async fn collect_generations(
    pool: &PgPool,
    config: CollectionConfig,
    adoption: &AdoptionState,
    now: DateTime<Utc>,
) -> Result<CollectReport, sqlx::Error> {
    let mut report = CollectReport::default();

    // The candidate is the newest published generation; collection runs only
    // when the API confirmed adopting exactly that reference.
    let newest = match super::adopt::newest_published(pool).await? {
        Some(newest) => newest,
        None => {
            report.deferred_unadopted = true;
            return Ok(report);
        }
    };
    if adoption.generation_id != newest.generation_id {
        report.deferred_unadopted = true;
        return Ok(report);
    }

    // Retention by recency: the newest `retention` manifest rows are kept
    // regardless of status (the in-flight candidate being built and the
    // previous recoverable generation live among them).
    let rows = sqlx::query!(
        "SELECT generation_id, status AS \"status!\", \
                retired_at AS \"retired_at\", created_at AS \"created_at!\" \
         FROM catalog_generations \
         ORDER BY COALESCE(published_at, created_at) DESC, created_at DESC, generation_id DESC",
    )
    .fetch_all(pool)
    .await?;
    for candidate in rows.into_iter().skip(config.retention) {
        let id = candidate.generation_id;
        // Only published generations are collected; a never-published
        // candidate keeps its artifacts until its own slice's semantics.
        if candidate.status != "published" {
            continue;
        }
        // Belt over the recency skip: the active generation is never
        // collected; an already-retired one is skipped (idempotency).
        if id == adoption.generation_id || candidate.retired_at.is_some() {
            continue;
        }
        // In-flight drain: defer while a holder is reported and the window
        // is still open; the window passing releases the projection.
        let held = adoption.in_flight.contains(&id);
        if held && (now - adoption.adopted_at) <= chrono_duration(config.retention_window) {
            report.deferred_in_flight.push(id);
            continue;
        }
        report.deleted_rows += collect_projections(pool, id).await?;
        sqlx::query!(
            "UPDATE catalog_generations SET retired_at = now() \
             WHERE generation_id = $1 AND retired_at IS NULL",
            id,
        )
        .execute(pool)
        .await?;
        report.collected.push(id);
    }
    Ok(report)
}

/// Deletes one generation's rows from every `generation_*` projection table
/// and returns the number of deleted rows. One statement per projection
/// table; the immutable projections are the only thing collection removes
/// (the manifest row stays as data, stamped with `retired_at`).
async fn collect_projections(pool: &PgPool, generation_id: Uuid) -> Result<u64, sqlx::Error> {
    let mut deleted = 0;
    for table in PROJECTION_TABLES {
        // Audited: the table name is a fixed allowlist constant above, never
        // input; the sqlx 0.9 safety assertion is required for dynamic SQL.
        let statement =
            sqlx::AssertSqlSafe(format!("DELETE FROM {table} WHERE generation_id = $1"));
        let affected = sqlx::query(statement)
            .bind(generation_id)
            .execute(pool)
            .await?
            .rows_affected();
        deleted += affected;
    }
    Ok(deleted)
}

/// The five per-generation projection tables (migration 0015). Names are a
/// fixed allowlist, never input.
const PROJECTION_TABLES: [&str; 5] = [
    "generation_life_events",
    "generation_fts_text",
    "generation_trigram_surface",
    "generation_event_cards",
    "generation_procedure_details",
];

fn chrono_duration(window: Duration) -> chrono::Duration {
    // Justified: `from_std` fails only for a window beyond chrono's range
    // (centuries); such a configuration saturates to the maximum instead of
    // panicking the worker's reconcile pass.
    chrono::Duration::from_std(window).unwrap_or(chrono::Duration::MAX)
}
