//! Internal readiness probe (S7 task 22, api delta cold-start, design §8):
//! `GET /ready` — deliberately OUTSIDE the closed `/api/v1` inventory. It
//! reports whether a valid catalog snapshot is loaded, and when it is, the
//! active generation's id, its age, and the last successful sync. Without a
//! valid snapshot the route answers 503 so a reverse proxy (S13) can hold
//! traffic back until the first load completes.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn ready(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    // Design §1.4: the request's first operation captures its generation —
    // the readiness report is coherent with exactly one snapshot.
    let generation = state.active.load_full();
    let Some(manifest) = generation.manifest() else {
        // Cold start: never ready before the first valid load.
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "starting",
                "generation": serde_json::Value::Null,
                "last_successful_sync": serde_json::Value::Null,
            })),
        ));
    };
    let age_seconds = (Utc::now() - manifest.published_at).num_seconds().max(0);
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ready",
            "generation": {
                "generation_id": manifest.generation_id.to_string(),
                "content_hash": manifest.content_hash,
                "published_at": manifest.published_at.to_rfc3339(),
                "age_seconds": age_seconds,
            },
            "last_successful_sync": manifest.source_synced_at.to_rfc3339(),
        })),
    ))
}
