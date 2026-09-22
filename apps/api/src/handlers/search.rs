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
use db::providers::generation_trigram::GenerationTrigramProvider;
use db::providers::orchestrator::ProviderFetch;
use db::providers::trigram::TrigramProvider;
use db::repos::search_log::{self, NewSearchLog};
use search::engine::CandidateProvider;
use search::normalizer::normalize;
use search::types::{Candidate, NormalizedQuery, ScoredEvent, SearchOutcome, SelectionMode};

use crate::cache::{self, CacheKey, CachedEntry};
use crate::dto;
use crate::error::ApiError;
use crate::generation::ActiveGeneration;
use crate::metrics::CacheEvent;
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
    let (outcome, cache_write, provider_ops) =
        lookup_or_compute(&state, &generation, &query).await?;
    // Task 1: the provider statements are consumed by the pipeline — ZERO
    // on a cache hit (no providers ran), the generation's provider cost on
    // a compute.
    state.metrics.observe_sql_ops(ROUTE, provider_ops);

    let log_ops = persist_log(&state, &query, &outcome).await?;
    state.metrics.observe_sql_ops(ROUTE, log_ops);

    // S9 task 28: the computation enters the cache only after the whole
    // request succeeds (the log persistence is part of it), so a search
    // that ends in a structural error caches nothing (search-cache delta).
    cache_write_commit(cache_write, &generation);

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
    let (outcome, cache_write, provider_ops) =
        lookup_or_compute(&state, &generation, &query).await?;
    state.metrics.observe_sql_ops(ROUTE, provider_ops);

    let log_ops = persist_log(&state, &query, &outcome).await?;
    state.metrics.observe_sql_ops(ROUTE, log_ops);

    cache_write_commit(cache_write, &generation);

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

/// The pending cache write of one compute-path request (S9 tasks 27/28):
/// the computation is inserted into the CAPTURED generation's cache only
/// after the request's log persists, so a structural error never leaves a
/// cached entry behind. A cache hit carries nothing to write.
enum CacheWrite {
    None,
    Pending {
        key: CacheKey,
        // Boxed: the cached computation is heap-sized and the write is
        // optional, so the none variant must not pay its size.
        entry: Box<CachedEntry>,
    },
}

/// Commits a pending cache write into the captured generation's cache
/// (no-op on a cache hit). Oversized single results are dropped inside
/// `SearchCache::insert` and served uncached by construction.
fn cache_write_commit(write: CacheWrite, generation: &ActiveGeneration) {
    match write {
        CacheWrite::None => {}
        CacheWrite::Pending { key, entry } => generation.cache.insert(key, *entry),
    }
}

/// The search pipeline with the S9 cache in front (task 27/28): a cache
/// hit rebuilds the outcome from the CURRENT request's text (its own
/// `query`, `normalized_query`, and tokens — never another request's);
/// a miss computes normalize → providers → score and hands back the
/// pending write. Provider/log errors stay structural (public 500) and
/// cache nothing.
async fn lookup_or_compute(
    state: &AppState,
    generation: &ActiveGeneration,
    query: &str,
) -> Result<(SearchOutcome, CacheWrite, u64), ApiError> {
    // Design §3.2: the key hashes the EFFECTIVE trimmed q the engine
    // receives — the same string `SearchEngine::normalize` gets. The
    // fingerprint lives only inside this in-memory key (R2/R14).
    let key = CacheKey::new(
        generation.generation_id(),
        generation.engine_version().to_string(),
        query,
    );
    if let Some(entry) = generation.cache.get(&key) {
        state.metrics.observe_cache(CacheEvent::Hit);
        return Ok((
            cache::rebuild(&generation.engine, query, &entry),
            CacheWrite::None,
            0,
        ));
    }

    state.metrics.observe_cache(CacheEvent::Miss);
    let normalized = generation.engine.normalize(query);
    let candidates = fetch_candidates(state, generation, &normalized).await?;
    let outcome = generation.engine.score(&normalized, candidates.clone());
    let entry = CachedEntry::from_outcome(&outcome, candidates);
    Ok((
        outcome,
        CacheWrite::Pending {
            key,
            entry: Box::new(entry),
        },
        provider_sql_ops(generation),
    ))
}

/// Fetches the FTS + trigram candidates for an already-normalized query
/// over the CAPTURED generation (S7 task 21), mirroring the db-side
/// orchestrator's fetch policy (S4b task 11) WITHOUT the scoring step:
/// the cache needs the raw candidates to store them alongside the ranked
/// result (task 28), so the decomposition lives at this boundary instead
/// of `orchestrator::run_search`. Provider errors stay structural.
async fn fetch_candidates(
    state: &AppState,
    generation: &ActiveGeneration,
    normalized: &NormalizedQuery,
) -> Result<Vec<Candidate>, ApiError> {
    let fts = FtsProvider::new(state.pool.clone());
    let candidates = match generation.is_loaded() {
        true => {
            let trigram = GenerationTrigramProvider::new(state.pool.clone());
            fetch_candidates_with(&fts, &trigram, generation, normalized, state.provider_fetch)
                .await?
        }
        false => {
            let trigram = TrigramProvider::new(state.pool.clone());
            fetch_candidates_with(&fts, &trigram, generation, normalized, state.provider_fetch)
                .await?
        }
    };
    Ok(candidates)
}

/// The provider fetch composition for one concrete provider pair
/// (sequential default, config-gated concurrent join). The canonical
/// candidate ordering happens inside `SearchEngine::score`, so fetch
/// order never reaches the ranking.
async fn fetch_candidates_with<F: CandidateProvider, T: CandidateProvider>(
    fts: &F,
    trigram: &T,
    generation: &ActiveGeneration,
    normalized: &NormalizedQuery,
    fetch: ProviderFetch,
) -> Result<Vec<Candidate>, ApiError> {
    let generation_id = generation.generation_id();
    let candidates = match fetch {
        ProviderFetch::Sequential => {
            let mut candidates = fts
                .candidates(generation_id, normalized)
                .await
                .map_err(provider_failure)?;
            let trigram_candidates = trigram
                .candidates(generation_id, normalized)
                .await
                .map_err(provider_failure)?;
            candidates.extend(trigram_candidates);
            candidates
        }
        ProviderFetch::Concurrent => {
            let (fts_candidates, trigram_candidates) = tokio::join!(
                fts.candidates(generation_id, normalized),
                trigram.candidates(generation_id, normalized),
            );
            let mut candidates = fts_candidates.map_err(provider_failure)?;
            candidates.extend(trigram_candidates.map_err(provider_failure)?);
            candidates
        }
    };
    Ok(candidates)
}

/// Maps a provider failure to the SAME structural error the pre-cache
/// pipeline produced (search-engine delta: provider failure is structural,
/// never a partial ranking).
fn provider_failure(error: search::engine::EngineError) -> ApiError {
    ApiError::InternalServerError(format!("search pipeline failed: {error}"))
}

/// The provider statements the pipeline issues per captured generation
/// (task 1 counts them at the call site): 2 on the legacy no-snapshot path
/// (FTS + trigram), 3 with a loaded snapshot — the generation-scoped trigram
/// provider sets its transaction-local similarity threshold with one
/// `set_config` statement before the precomputed-surface query (design
/// §2.2). S8's provider consolidation revisits the ≤3 final budget.
fn provider_sql_ops(generation: &ActiveGeneration) -> u64 {
    if generation.is_loaded() { 3 } else { 2 }
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

    // S7 task 21: a loaded generation serves the open cards from the
    // snapshot (0 SQL); the legacy `cards_by_event` query stays as the
    // no-snapshot path (exactly 1 statement) until the snapshot route is
    // fully verified.
    let procedures = match generation.cards(&slug) {
        Some(cards) => {
            state.metrics.observe_sql_ops(ROUTE, 0);
            dto::procedure_cards_from_event_cards(crate::generation::cloned_cards(&cards))
        }
        None => match db::repos::procedures::cards_by_event(&state.pool, &slug).await {
            Ok(Some(cards)) => {
                // Task 7: the transition cards query issues exactly 1
                // statement (no event metadata, no raw_data transport).
                state.metrics.observe_sql_ops(ROUTE, 1);
                dto::procedure_cards_from_event_cards(cards)
            }
            Ok(None) => Vec::new(),
            Err(error) => {
                return Err(ApiError::InternalServerError(format!(
                    "event procedures query failed: {error}"
                )));
            }
        },
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
