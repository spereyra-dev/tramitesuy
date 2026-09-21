//! Category endpoints (task 72, API-7): `GET /categories` lists slug, name,
//! and `order_index` ascending (vehiculos first); `GET
//! /categories/:slug/events` lists that category's events with slug and
//! name; an unknown category slug returns 404.
//!
//! S7 task 20: both reads serve the CAPTURED generation snapshot — zero
//! catalog SQL. Before the first valid snapshot load the cold-start gate
//! (api delta) returns 503.

use axum::Json;
use axum::extract::{Path, State};

use crate::dto;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list(State(state): State<AppState>) -> Result<Json<dto::CategoriesPage>, ApiError> {
    // Design §1.4: the request's first operation captures its generation.
    let generation = state.active.load_full();
    if !generation.is_loaded() {
        // Cold start: catalog reads 503 until the first valid load (api
        // delta, S7 task 22).
        return Err(ApiError::ColdStart);
    }
    let categories = &generation.categories();
    state.metrics.observe_sql_ops("/api/v1/categories", 0);
    Ok(Json(dto::CategoriesPage {
        categories: categories
            .iter()
            .map(|category| dto::CategoryPage {
                slug: category.slug.clone(),
                name: category.name.clone(),
                order_index: category.order_index,
            })
            .collect(),
    }))
}

pub async fn events(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<dto::CategoryEventsPage>, ApiError> {
    let generation = state.active.load_full();
    let known = generation
        .categories()
        .iter()
        .any(|category| category.slug == slug);
    if !known {
        return Err(if generation.is_loaded() {
            ApiError::NotFound
        } else {
            ApiError::ColdStart
        });
    }
    let events = generation.events_of_category(&slug);
    state
        .metrics
        .observe_sql_ops("/api/v1/categories/{slug}/events", 0);
    Ok(Json(dto::CategoryEventsPage {
        category: slug,
        events: events
            .iter()
            .map(|event| dto::EventSummaryPage {
                slug: event.slug.clone(),
                name: event.name.clone(),
            })
            .collect(),
    }))
}
