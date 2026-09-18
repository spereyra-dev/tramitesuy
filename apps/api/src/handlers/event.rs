//! `GET /events/:slug` (task 71, API-6): the event's name, description,
//! category, and its procedures ordered by `order_index` with order,
//! required, official_url, and the attribution block; unknown slug → 404.
//! C1a registers the route; the read implementation lands in C1b.

use crate::error::ApiError;

pub async fn get() -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /events/:slug read handler lands with task 71 (C1b)".to_string(),
    ))
}
