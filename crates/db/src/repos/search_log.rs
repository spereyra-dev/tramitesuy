//! Search-log repository (API-10, task 82): persists exactly the specced
//! telemetry — the ALREADY-redacted query, its normalized form, the
//! selected/top event ids (nullable FKs), the top score, and the timestamp
//! (`created_at` defaults to `now()`). No IP, user agent, name, or contact
//! data exists in the schema (allowlist-tested in `tests/search_log.rs`).
//!
//! Event slugs are resolved to `life_events` ids INSIDE the insert statement
//! (task 6, OPT-06/OPT-09: the whole log path costs exactly one statement);
//! an event absent from the DB projection (e.g. a test-only YAML event)
//! stores NULL ids rather than failing the search.

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

/// Inserts one log row and returns its generated id. Both event slugs
/// resolve through scalar subqueries inside the single statement (task 6):
/// a present slug resolves its `life_events` id, an absent slug (or a NULL
/// slug) yields NULL — exactly the behavior of the previous per-slug
/// lookups, in one statement instead of three.
pub async fn insert(pool: &PgPool, log: &NewSearchLog) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query!(
        "INSERT INTO search_logs \
             (query, normalized_query, selected_event_id, top_event_id, top_score) \
         SELECT $1, $2, \
                (SELECT id FROM life_events WHERE slug = $3), \
                (SELECT id FROM life_events WHERE slug = $4), \
                $5 \
         RETURNING id",
        log.query,
        log.normalized_query,
        log.selected_event_slug,
        log.top_event_slug,
        log.top_score.map(|score| score as f64),
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}
