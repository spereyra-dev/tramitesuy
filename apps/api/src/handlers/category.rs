//! Category endpoints (task 72, API-7): `GET /categories` lists slug, name,
//! and `order_index` ascending (vehiculos first); `GET
//! /categories/:slug/events` lists that category's events with slug and
//! name; unknown slug → 404. C1a registers the routes; the read
//! implementation lands in C1b.

use crate::error::ApiError;

pub async fn list() -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /categories read handler lands with task 72 (C1b)".to_string(),
    ))
}

pub async fn events() -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /categories/:slug/events read handler lands with task 72 (C1b)".to_string(),
    ))
}
