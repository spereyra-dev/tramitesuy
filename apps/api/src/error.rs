//! `ApiError` → HTTP mapping (task 70, design §3 error strategy): 404 for
//! unknown slug/id, 400 for invalid request payloads, 500 for everything
//! else. Internal causes are logged on the server and never serialized into
//! a response body (leak-none clause).

use axum::Json;
use axum::http::header::RETRY_AFTER;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub enum ApiError {
    /// Unknown slug or id (public: safe to 404 without a body).
    NotFound,
    /// Malformed request payload; the detail names the reason server-side.
    BadRequest(String),
    /// Cold start (S7 task 22, api delta): no valid catalog snapshot is
    /// loaded yet, so catalog reads refuse traffic until the first valid
    /// load completes.
    ColdStart,
    /// Storage or handler failure; the detail is logged, never returned.
    InternalServerError(String),
    /// S12 tasks 38/39 (design §7.2): the admission limiter is saturated
    /// or the connection pool is exhausted within its acquire timeout.
    /// Answers 503 with a `Retry-After` header and the documented public
    /// body — distinct from the deadline 504, and never a 429 (the API
    /// does not invent rate-limit codes; an explicit proxy policy may).
    Overloaded { retry_after_seconds: u64 },
}

impl ApiError {
    /// The shared overload constructor (task 39 GREEN: one home for the
    /// overload shape). `retry_after_seconds` is configuration-driven
    /// (`ApiLimits::retry_after_seconds`) — never a hardcoded constant.
    pub fn overload(retry_after_seconds: u64) -> ApiError {
        ApiError::Overloaded {
            retry_after_seconds,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::NotFound => public_error(StatusCode::NOT_FOUND, "not found"),
            ApiError::BadRequest(detail) => {
                eprintln!("api bad request: {detail}");
                public_error(StatusCode::BAD_REQUEST, "bad request")
            }
            ApiError::ColdStart => {
                public_error(StatusCode::SERVICE_UNAVAILABLE, "service starting")
            }
            ApiError::InternalServerError(detail) => {
                eprintln!("api internal error: {detail}");
                public_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            }
            ApiError::Overloaded {
                retry_after_seconds,
            } => {
                eprintln!("api overloaded: admission or pool limit saturated");
                let mut response = public_error(StatusCode::SERVICE_UNAVAILABLE, "overloaded");
                if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
                    response.headers_mut().insert(RETRY_AFTER, value);
                }
                response
            }
        }
    }
}

/// The single public error shape: `{"error": "<public message>"}` — no
/// internal details, no request echo.
fn public_error(status: StatusCode, message: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
