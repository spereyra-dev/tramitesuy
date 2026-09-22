//! Adoption record (S8 task 23, design §2.3): the durable handshake between
//! the API and the worker. After every swap the API writes the adopted
//! generation's manifest row — `active_generation_id` + `adopted_at`, plus
//! the set of generations still held by in-flight requests — and the
//! worker's reconciler reads it back.
//!
//! Adoption confirmation is the publication-acceptance signal: the worker
//! considers a publication adopted only when the manifest says so (never the
//! in-memory swap alone, which lives in another process). A lagging API is
//! observable as a newest-published row with no adoption record.

use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

/// The latest adoption record the API has written: which generation is
/// served, when it was adopted, and which generations are still in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionState {
    pub generation_id: Uuid,
    pub adopted_at: DateTime<Utc>,
    pub in_flight: Vec<Uuid>,
}

/// The newest published manifest row: the adoption candidate the API must
/// detect and load (reconciliation never reads `building`/`validated`
/// candidates — publication is the only trigger).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedReference {
    pub generation_id: Uuid,
    pub published_at: DateTime<Utc>,
    pub event_count: i32,
    pub procedure_count: i32,
}

/// Writes the adoption record onto one manifest row (the API's write-back
/// after a swap): the row self-identifies as the active generation with its
/// adoption timestamp, plus the in-flight generation ids. A rollback
/// re-adoption writes an older generation's row again — the latest
/// `adopted_at` row is the current adoption (see [`latest_adoption`]).
pub async fn confirm_adoption(
    pool: &PgPool,
    generation_id: Uuid,
    in_flight: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE catalog_generations \
         SET active_generation_id = $1, adopted_at = now(), inflight_generation_ids = $2 \
         WHERE generation_id = $1",
        generation_id,
        in_flight,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The current adoption record: the manifest row with the latest
/// `adopted_at`. `None` means nothing has ever been adopted (a lagging API
/// that has not detected any publication yet).
pub async fn latest_adoption(pool: &PgPool) -> Result<Option<AdoptionState>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT generation_id, adopted_at AS \"adopted_at!\", inflight_generation_ids \
         FROM catalog_generations \
         WHERE active_generation_id IS NOT NULL AND adopted_at IS NOT NULL \
         ORDER BY adopted_at DESC \
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| AdoptionState {
        generation_id: row.generation_id,
        adopted_at: row.adopted_at,
        in_flight: row.inflight_generation_ids,
    }))
}

/// The newest published generation (the candidate the API must adopt).
/// Only rows that were actually promoted (`published` + a promotion stamp)
/// qualify — an interrupted build never becomes the candidate (S6/S7
/// contract). The counts ride along so an adopting API can size its
/// memory-budget projection without a second query.
pub async fn newest_published(pool: &PgPool) -> Result<Option<PublishedReference>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT generation_id, published_at AS \"published_at!\", \
                event_count, procedure_count \
         FROM catalog_generations \
         WHERE status = 'published' AND published_at IS NOT NULL \
         ORDER BY published_at DESC, created_at DESC \
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| PublishedReference {
        generation_id: row.generation_id,
        published_at: row.published_at,
        event_count: row.event_count,
        procedure_count: row.procedure_count,
    }))
}

/// Reactivates a retained published generation (the rollback path,
/// catalog-generations delta "Previous generation remains recoverable"):
/// the rollback is a re-promotion of the retained previous generation — the
/// defective generation is never mutated to fix it. Only a `published` row
/// can be reactivated; returns whether the re-promotion happened.
pub async fn reactivate(pool: &PgPool, generation_id: Uuid) -> Result<bool, sqlx::Error> {
    let promoted = sqlx::query_scalar!(
        "UPDATE catalog_generations SET published_at = now() \
         WHERE generation_id = $1 AND status = 'published' \
         RETURNING published_at AS \"published_at!\"",
        generation_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(promoted.is_some())
}
