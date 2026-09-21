//! Category endpoints (task 72, API-7): `GET /categories` lists slug, name,
//! and `order_index` ascending (vehiculos first); `GET
//! /categories/:slug/events` lists that category's events with slug and
//! name; an unknown category slug returns 404.

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list(State(state): State<AppState>) -> Result<Json<dto::CategoriesPage>, ApiError> {
    let rows = db::repos::taxonomy_seed::categories(&state.pool)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("categories query failed: {e}")))?;
    state.metrics.observe_sql_ops("/api/v1/categories", 1);
    Ok(Json(dto::categories_page(rows)))
}

pub async fn events(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<dto::CategoryEventsPage>, ApiError> {
    let rows = db::repos::taxonomy_seed::events_by_category(&state.pool, &slug)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("category events query failed: {e}")))?;
    match rows {
        Some(rows) => {
            state
                .metrics
                .observe_sql_ops("/api/v1/categories/{slug}/events", 1);
            Ok(Json(dto::category_events_page(slug, rows)))
        }
        None => Err(ApiError::NotFound),
    }
}
