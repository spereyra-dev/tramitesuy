//! Search-feedback repository (API-9, task 86): the single write path for
//! the `search_feedback` table (DM-1 table 10). One insert, linked to the
//! search log and the event by foreign key — no UI, no update, no delete
//! path exists (D-3: write path only).

use sqlx::PgPool;
use sqlx::types::Uuid;

/// One new feedback submission, already validated at the handler boundary.
#[derive(Debug, Clone)]
pub struct NewFeedback {
    pub search_log_id: Uuid,
    pub event_id: Uuid,
    pub correct: bool,
}

/// Inserts one feedback row and returns its generated id.
///
/// A foreign-key violation (`23503`) means the caller submitted an unknown
/// `search_log_id` or `event_id`; the handler maps that SQLSTATE to a
/// public 400.
pub async fn insert(pool: &PgPool, feedback: &NewFeedback) -> Result<Uuid, sqlx::Error> {
    let row = sqlx::query!(
        "INSERT INTO search_feedback (search_log_id, event_id, correct) \
         VALUES ($1, $2, $3) \
         RETURNING id",
        feedback.search_log_id,
        feedback.event_id,
        feedback.correct,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}
