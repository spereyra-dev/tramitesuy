//! `POST /search/feedback` — the MVP's only write endpoint (API-9, task 86,
//! D-3). Accepts `{search_log_id, event_id, correct}` and persists one
//! `search_feedback` row linked to the search log and the event. Valid
//! submissions return 201; an unknown `search_log_id` or `event_id` (FK
//! violation) returns 400. This is the write path only — no feedback UI is
//! part of this change.
//!
//! The body is parsed manually from raw bytes so every malformed payload
//! maps to the exact public error body through `ApiError` (no axum
//! extractor rejection text can leak into a response).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;

/// The wire shape: ids arrive as UUID strings and are parsed explicitly so
/// a malformed id is a clean public 400 (not a serde type error).
#[derive(Debug, Deserialize)]
struct FeedbackRequest {
    search_log_id: String,
    event_id: String,
    correct: bool,
}

impl FeedbackRequest {
    fn parse(self) -> Result<db::repos::search_feedback::NewFeedback, String> {
        Ok(db::repos::search_feedback::NewFeedback {
            search_log_id: self
                .search_log_id
                .parse()
                .map_err(|_| format!("search_log_id is not a UUID: {:?}", self.search_log_id))?,
            event_id: self
                .event_id
                .parse()
                .map_err(|_| format!("event_id is not a UUID: {:?}", self.event_id))?,
            correct: self.correct,
        })
    }
}

pub async fn create(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let payload: FeedbackRequest = serde_json::from_slice(&body)
        .map_err(|error| ApiError::BadRequest(format!("invalid feedback body: {error}")))?;
    let feedback = payload.parse().map_err(ApiError::BadRequest)?;

    db::repos::search_feedback::insert(&state.pool, &feedback)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23503") => {
                ApiError::BadRequest(format!("unknown search_log_id or event_id: {db_err}"))
            }
            _ => ApiError::InternalServerError(format!("feedback persistence failed: {error}")),
        })?;
    state.metrics.observe_sql_ops("/api/v1/search/feedback", 1);

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"status": "created"})),
    ))
}
