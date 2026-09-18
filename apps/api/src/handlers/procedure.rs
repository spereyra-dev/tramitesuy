//! `GET /procedures/:id` (task 73, API-8): name, description, organization,
//! official_url, cost fields per the missing-cost rule, status, and the
//! attribution block; an inactive procedure stays fetchable with
//! `status: "inactive"`. C1a registers the route; the read implementation
//! lands in C1b.

use crate::error::ApiError;

pub async fn get() -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "GET /procedures/:id read handler lands with task 73 (C1b)".to_string(),
    ))
}
