//! `POST /search/feedback` — registered in C1 as a route slot (task 70,
//! API-1 closed inventory). The write path (FK validation → 201/400) is
//! unit C3 (task 86); until then the slot answers with the public 500 shape
//! through `ApiError` — never a fabricated success, never a 404.

use crate::error::ApiError;

pub async fn create() -> Result<(), ApiError> {
    Err(ApiError::InternalServerError(
        "POST /search/feedback is wired in unit C3 (task 86)".to_string(),
    ))
}
