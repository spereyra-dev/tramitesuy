//! `GET /procedures/:id` (task 73, API-8): name, description, organization,
//! official_url, cost fields per the missing-cost rule, status, and the
//! attribution block. An inactive procedure remains fetchable and reports
//! `status: "inactive"` with its attribution intact.

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<dto::ProcedureDetailPage>, ApiError> {
    let detail = db::repos::procedures::by_external_id(&state.pool, &id)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("procedure query failed: {e}")))?;
    match detail {
        Some(detail) => Ok(Json(dto::procedure_detail_page(detail))),
        None => Err(ApiError::NotFound),
    }
}
