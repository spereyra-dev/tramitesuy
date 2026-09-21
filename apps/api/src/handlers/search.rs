//! `GET /search` and `GET /search/debug` (API-2/API-5, tasks 79–80): the
//! full search pipeline from design §4.2 —
//! redact (log copy) → db-side async orchestrator over the YAML-fed engine
//! with DB-backed FTS/trigram providers → persist the redacted `search_logs`
//! row (task 82) → JSON per the mode shape.
//!
//! The engine's lexicons come from the YAML taxonomy cached in `AppState`
//! (task 84); the providers query the DB projection. A failing provider or
//! a failing log insert is a structural error → public 500 (design §3).

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Query, State};
use db::providers::fts::FtsProvider;
use db::providers::orchestrator;
use db::providers::trigram::TrigramProvider;
use db::repos::search_log::{self, NewSearchLog};
use search::normalizer::normalize;
use search::types::{ScoredEvent, SearchOutcome, SelectionMode};

use crate::dto;
use crate::error::ApiError;
use crate::generation::ActiveGeneration;
use crate::redaction::redact;
use crate::state::AppState;

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Design §1.4: the request's FIRST operation captures its generation;
    // payload, log, and providers all use this one `Arc` (task 20).
    let generation = state.active.load_full();
    let query = query_parameter(&params)?;
    let outcome = run_pipeline(&state, &generation, &query).await?;
    // Task 1: the two provider statements are consumed by the pipeline.
    state.metrics.observe_sql_ops(ROUTE, 2);

    let log_ops = persist_log(&state, &query, &outcome).await?;
    state.metrics.observe_sql_ops(ROUTE, log_ops);

    let payload = match outcome.selection.mode {
        SelectionMode::Open => open_payload(&state, &generation, &outcome).await?,
        SelectionMode::Disambiguation => disambiguation_payload(&generation, &outcome),
        SelectionMode::Categories => categories_payload(&generation, &outcome),
    };
    Ok(Json(payload))
}

/// This route's low-cardinality metrics label (task 1): the route pattern,
/// never the query text (R14).
const ROUTE: &str = "/api/v1/search";

pub async fn debug(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let generation = state.active.load_full();
    let query = query_parameter(&params)?;
    let outcome = run_pipeline(&state, &generation, &query).await?;
    state.metrics.observe_sql_ops(ROUTE, 2);

    let log_ops = persist_log(&state, &query, &outcome).await?;
    state.metrics.observe_sql_ops(ROUTE, log_ops);

    Ok(Json(debug_payload(&generation, &outcome)))
}

/// Extracts the required `q` parameter (missing or blank → 400).
fn query_parameter(params: &HashMap<String, String>) -> Result<String, ApiError> {
    params
        .get("q")
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
        .ok_or_else(|| ApiError::BadRequest("missing or empty q parameter".to_string()))
}

/// Runs the deterministic pipeline through the db-side async orchestration
/// boundary (S4b task 11) over the CAPTURED generation: its engine ranks,
/// and the request's captured generation id travels with the provider
/// contract (the legacy tables are not generation-scoped until the stage-3
/// providers land; task 21 scopes the trigram provider). Provider errors
/// stay structural and map to the existing public 500 path; no partial
/// candidate ranking is produced.
async fn run_pipeline(
    state: &AppState,
    generation: &ActiveGeneration,
    query: &str,
) -> Result<SearchOutcome, ApiError> {
    let fts = FtsProvider::new(state.pool.clone());
    let trigram = TrigramProvider::new(state.pool.clone());
    orchestrator::run_search(
        &generation.engine,
        generation.generation_id(),
        query,
        &fts,
        &trigram,
        state.provider_fetch,
    )
    .await
    .map_err(|error| ApiError::InternalServerError(format!("search pipeline failed: {error}")))
}

/// Persists the redacted log row (API-10, task 82): ONLY the redacted query,
/// the normalized form of the redacted query, the selected/top event slugs
/// (resolved to nullable ids by the repository), the top score, and the
/// timestamp. The raw query is never handed to the repository.
async fn persist_log(
    state: &AppState,
    query: &str,
    outcome: &SearchOutcome,
) -> Result<u64, ApiError> {
    let redacted = redact(query);
    let normalized = normalize(&redacted).normalized;
    let selected_event_slug = match outcome.selection.mode {
        SelectionMode::Open => outcome.selection.event_slug.clone(),
        _ => None,
    };
    let top_event_slug = outcome.results.first().map(|event| event.slug.clone());
    let top_score = outcome.results.first().map(|event| event.score);
    // Task 6: the consolidated insert resolves both slugs inline — the log
    // path is exactly one statement.
    let sql_ops = 1;
    search_log::insert(
        &state.pool,
        &NewSearchLog {
            query: redacted,
            normalized_query: normalized,
            selected_event_slug,
            top_event_slug,
            top_score,
        },
    )
    .await
    .map(|_| ())
    .map_err(|error| {
        ApiError::InternalServerError(format!("search log persistence failed: {error}"))
    })?;
    Ok(sql_ops)
}

/// Open mode (API-2): `results` carries exactly the selected event — slug,
/// name, score, confidence — plus the event's ordered procedures summary
/// with the attribution block (API-4). An event absent from the DB
/// projection serves an empty procedures summary. The names resolve from
/// the CAPTURED generation's slug maps (task 19 GREEN); task 21 serves the
/// cards from the snapshot.
async fn open_payload(
    state: &AppState,
    generation: &ActiveGeneration,
    outcome: &SearchOutcome,
) -> Result<serde_json::Value, ApiError> {
    let slug = outcome
        .selection
        .event_slug
        .clone()
        // Justified inline: the engine's Open selection is constructed only
        // together with its event slug — a mode with no selected event is
        // unreachable by construction (search-engine invariant).
        .expect("open selection always names the selected event");
    let top = outcome
        .results
        .first()
        // Justified inline: Open mode ranks at least one candidate into
        // `results` (the engine always emits the selected event first), so
        // the list cannot be empty in this mode.
        .expect("open selection implies a top result");

    // Task 21 replaces this with snapshot cards for the loaded-snapshot
    // path; until then `cards_by_event` stays the no-snapshot path.
    let procedures = match db::repos::procedures::cards_by_event(&state.pool, &slug).await {
        Ok(Some(cards)) => {
            // Task 7: the transition cards query issues exactly 1 statement
            // (no event metadata, no raw_data transport).
            state.metrics.observe_sql_ops(ROUTE, 1);
            dto::procedure_cards_from_event_cards(cards)
        }
        Ok(None) => Vec::new(),
        Err(error) => {
            return Err(ApiError::InternalServerError(format!(
                "event procedures query failed: {error}"
            )));
        }
    };

    Ok(serde_json::json!({
        "query": outcome.query.original,
        "normalized_query": outcome.query.normalized,
        "mode": "open",
        "confidence": outcome.confidence,
        "results": [{
            "event": {
                "slug": slug,
                "name": generation.event_name(&slug).unwrap_or_default(),
            },
            "score": top.score,
            "confidence": outcome.confidence,
            "procedures": procedures,
        }],
    }))
}

/// Disambiguation mode (API-2, SE-10): up to 3 top-scored events, no single
/// answer. Names resolve from the YAML taxonomy.
fn disambiguation_payload(
    generation: &ActiveGeneration,
    outcome: &SearchOutcome,
) -> serde_json::Value {
    let options: Vec<serde_json::Value> = outcome
        .selection
        .options
        .iter()
        .map(|event| option_payload(generation, event, outcome.confidence))
        .collect();
    serde_json::json!({
        "query": outcome.query.original,
        "normalized_query": outcome.query.normalized,
        "mode": "disambiguation",
        "confidence": outcome.confidence,
        "options": options,
    })
}

fn option_payload(
    generation: &ActiveGeneration,
    event: &ScoredEvent,
    confidence: f64,
) -> serde_json::Value {
    serde_json::json!({
        "slug": event.slug,
        "name": generation.event_name(&event.slug).unwrap_or_default(),
        "score": event.score,
        "confidence": confidence,
    })
}

/// Categories mode (API-2): the available category slugs with their YAML
/// names.
fn categories_payload(generation: &ActiveGeneration, outcome: &SearchOutcome) -> serde_json::Value {
    let categories: Vec<serde_json::Value> = outcome
        .selection
        .categories
        .iter()
        .map(|slug| {
            serde_json::json!({
                "slug": slug,
                "name": generation.category_name(slug).unwrap_or(slug),
            })
        })
        .collect();
    serde_json::json!({
        "query": outcome.query.original,
        "normalized_query": outcome.query.normalized,
        "mode": "categories",
        "confidence": outcome.confidence,
        "categories": categories,
    })
}

/// Debug mode (API-5, SE-11): tokens with original+canonical forms, and per
/// result an explanation array whose entry values sum exactly to the score.
/// Every result echoes the outcome-wide confidence (SE-9's formula is
/// computed from the full candidate list, not per event).
fn debug_payload(generation: &ActiveGeneration, outcome: &SearchOutcome) -> serde_json::Value {
    let tokens: Vec<serde_json::Value> = outcome
        .query
        .tokens
        .iter()
        .map(|token| {
            serde_json::json!({
                "original": token.original,
                "canonical": token.canonical,
            })
        })
        .collect();
    let results: Vec<serde_json::Value> = outcome
        .results
        .iter()
        .map(|event| {
            serde_json::json!({
                "slug": event.slug,
                "name": generation.event_name(&event.slug),
                "score": event.score,
                "confidence": outcome.confidence,
                "explanation": event
                    .explanation
                    .entries
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "rule": entry.rule_name,
                            "term": entry.term,
                            "canonical": entry.canonical,
                            "value": entry.value,
                        })
                    })
                    .collect::<Vec<serde_json::Value>>(),
            })
        })
        .collect();
    serde_json::json!({
        "query": outcome.query.original,
        "normalized_query": outcome.query.normalized,
        "tokens": tokens,
        "results": results,
    })
}
