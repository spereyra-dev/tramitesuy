//! `GET /procedures/:id` (task 73, API-8): name, description, organization,
//! official_url, cost fields per the missing-cost rule, status, and the
//! attribution block. An inactive procedure remains fetchable and reports
//! `status: "inactive"` with its attribution intact.
//!
//! S7 task 20: the detail serves the CAPTURED generation snapshot — zero
//! catalog SQL (an unknown id is 404 without any query, catalog-generations
//! delta). Before the first valid snapshot load the cold-start gate (api
//! delta) returns 503.

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

/// Field-wise clone of the snapshot's shared detail (immutable data; the
/// `crates/db` record carries no `Clone` outside this slice's surfaces).
fn cloned_detail(
    detail: &db::repos::procedures::ProcedureDetail,
) -> db::repos::procedures::ProcedureDetail {
    db::repos::procedures::ProcedureDetail {
        external_id: detail.external_id.clone(),
        name: detail.name.clone(),
        description: detail.description.clone(),
        organization_name: detail.organization_name.clone(),
        official_url: detail.official_url.clone(),
        status: detail.status.clone(),
        raw_data: detail.raw_data.clone(),
        last_seen_at: detail.last_seen_at,
    }
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<dto::ProcedureDetailPage>, ApiError> {
    // Design §1.4: the request's first operation captures its generation.
    let generation = state.active.load_full();
    if !generation.is_loaded() {
        return Err(ApiError::ColdStart);
    }
    let Some(detail) = generation.procedure(&id) else {
        return Err(ApiError::NotFound);
    };
    state.metrics.observe_sql_ops("/api/v1/procedures/{id}", 0);
    Ok(Json(dto::procedure_detail_page(cloned_detail(&detail))))
}
