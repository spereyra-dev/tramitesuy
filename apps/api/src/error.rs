//! `ApiError` → HTTP mapping (task 70, design §3 error strategy): 404 for
//! unknown slug/id, 400 for invalid request payloads, 500 for everything
//! else. Internal causes are logged on the server and never serialized into
//! a response body (leak-none clause).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub enum ApiError {
    /// Unknown slug or id (public: safe to 404 without a body).
    NotFound,
    /// Malformed request payload; the detail names the reason server-side.
    BadRequest(String),
    /// Storage or handler failure; the detail is logged, never returned.
    InternalServerError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::NotFound => public_error(StatusCode::NOT_FOUND, "not found"),
            ApiError::BadRequest(detail) => {
                eprintln!("api bad request: {detail}");
                public_error(StatusCode::BAD_REQUEST, "bad request")
            }
            ApiError::InternalServerError(detail) => {
                eprintln!("api internal error: {detail}");
                public_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            }
        }
    }
}

/// The single public error shape: `{"error": "<public message>"}` — no
/// internal details, no request echo.
fn public_error(status: StatusCode, message: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
