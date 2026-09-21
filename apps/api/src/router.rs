//! The `/api/v1` router (task 70, API-1): exactly the seven specced routes
//! under the closed inventory — no admin, auth, or account endpoints. The
//! read endpoints are unit C1; the search endpoints dial up in C2 (tasks
//! 79–82); the feedback slot stays registered and answers the public 500
//! until C3.

use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};

use crate::handlers;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Internal readiness probe (S7 task 22): deliberately OUTSIDE the
        // closed `/api/v1` inventory — the proxy uses it to hold traffic
        // back until the first valid snapshot load.
        .route("/ready", get(handlers::readiness::ready))
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            observe_request,
        ))
        .with_state(state)
}

/// Task 1 wiring: times every served request and reports route pattern +
/// status + wall latency to the metrics seam. The route label is the axum
/// route pattern (low cardinality, request-independent) — never the query.
async fn observe_request(
    axum::extract::State(state): axum::extract::State<AppState>,
    path: Option<MatchedPath>,
    request: Request,
    next: Next,
) -> Response {
    let route = path
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    state.metrics.observe_request(
        &route,
        response.status().as_u16(),
        started.elapsed().as_micros(),
    );
    response
}
