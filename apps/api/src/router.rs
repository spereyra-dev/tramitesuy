//! The `/api/v1` router (task 70, API-1): exactly the seven specced routes
//! under the closed inventory — no admin, auth, or account endpoints. The
//! read endpoints are unit C1; the search endpoints dial up in C2 (tasks
//! 79–82); the feedback slot stays registered and answers the public 500
//! until C3.

use axum::Router;
use axum::routing::{get, post};

use crate::handlers;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/search", get(handlers::search::search))
        .route("/api/v1/search/debug", get(handlers::search::debug))
        .route("/api/v1/search/feedback", post(handlers::feedback::create))
        .route("/api/v1/events/{slug}", get(handlers::event::get))
        .route("/api/v1/categories", get(handlers::category::list))
        .route(
            "/api/v1/categories/{slug}/events",
            get(handlers::category::events),
        )
        .route("/api/v1/procedures/{id}", get(handlers::procedure::get))
        .fallback(|| async { crate::error::ApiError::NotFound })
        .with_state(state)
}
