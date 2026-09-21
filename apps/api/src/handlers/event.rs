//! `GET /events/:slug` (task 71, API-6): the event's name, description,
//! category, and its procedures ordered by `order_index`, each carrying
//! `order`, `required`, `official_url`, the cost pair, and the attribution
//! block; an unknown slug returns 404. Delegated design note: design §7
//! sketches this handler on top of `taxonomy::loader`; the implementation
//! serves the seeded DB projection (`crates/db` `by_event`) instead — same
//! YAML-derived content, one query, and the projection is what the website
//! and FTS providers read (recorded in apply-progress).

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

/// This route's low-cardinality metrics label (task 1).
const ROUTE: &str = "/api/v1/events/{slug}";

pub async fn get(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<dto::EventPage>, ApiError> {
    let projection = db::repos::procedures::by_event(&state.pool, &slug)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("event query failed: {e}")))?;
    match projection {
        Some(projection) => {
            // Task 1: by_event issues 2 statements (metadata + rows).
            state.metrics.observe_sql_ops(ROUTE, 2);
            Ok(Json(dto::event_page(projection)))
        }
        None => Err(ApiError::NotFound),
    }
}
