//! Search-log repository (API-10, task 82): persists exactly the specced
//! telemetry — the ALREADY-redacted query, its normalized form, the
//! selected/top event ids (nullable FKs), the top score, and the timestamp
//! (`created_at` defaults to `now()`). No IP, user agent, name, or contact
//! data exists in the schema (allowlist-tested in `tests/search_log.rs`).
//!
//! Event slugs are resolved to `life_events` ids here; an event absent from
//! the DB projection (e.g. a test-only YAML event) stores NULL ids rather
//! than failing the search.

use sqlx::PgPool;
use sqlx::types::Uuid;

/// One new search-log row. `query` MUST already be redacted by the caller
/// (`apps/api::redaction::redact`) — this repository never sees the raw
/// query, so a leaked pattern cannot enter `search_logs` through it.
#[derive(Debug, Clone)]
pub struct NewSearchLog {
    pub query: String,
    pub normalized_query: String,
    pub selected_event_slug: Option<String>,
    pub top_event_slug: Option<String>,
    pub top_score: Option<i64>,
}

/// Inserts one log row and returns its generated id.
pub async fn insert(pool: &PgPool, log: &NewSearchLog) -> Result<Uuid, sqlx::Error> {
    let selected = event_id(pool, log.selected_event_slug.as_deref()).await?;
    let top = event_id(pool, log.top_event_slug.as_deref()).await?;
    let row = sqlx::query!(
        "INSERT INTO search_logs \
             (query, normalized_query, selected_event_id, top_event_id, top_score) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id",
        log.query,
        log.normalized_query,
        selected,
        top,
        log.top_score.map(|score| score as f64),
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

/// Resolves an event slug to its `life_events` id, or `None` when the event
/// is absent from the DB projection.
async fn event_id(pool: &PgPool, slug: Option<&str>) -> Result<Option<Uuid>, sqlx::Error> {
    match slug {
        None => Ok(None),
        Some(slug) => Ok(
            sqlx::query!("SELECT id FROM life_events WHERE slug = $1", slug)
                .fetch_optional(pool)
                .await?
                .map(|row| row.id),
        ),
    }
}
