//! `GET /search` and `GET /search/debug` — registered in C1 as route slots
//! (task 70, API-1 closed inventory). The search pipeline (engine, DB
//! candidate providers, redaction, search logging) is unit C2 (tasks
//! 78-85); until then the slots answer with the public 500 shape through
//! `ApiError` — never a fabricated payload, never a 404.

use std::collections::HashMap;

use axum::extract::{Query, State};

use crate::error::ApiError;
use crate::state::AppState;

pub async fn search(
    State(_state): State<AppState>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /search is wired in unit C2 (tasks 78-85)".to_string(),
    ))
}

pub async fn debug(
    State(_state): State<AppState>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /search/debug is wired in unit C2 (tasks 78-85)".to_string(),
    ))
}
