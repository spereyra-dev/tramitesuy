//! Async search orchestration layer (S4b task 11, design §4.2): the
//! database waiting that used to sit behind the synchronous
//! `block_in_place`/`block_on` bridge now happens here, awaited directly
//! from the async callers (the axum handlers). The pipeline:
//!
//! normalize → async providers → `SearchEngine::score` (canonical
//! candidate ordering happens inside `score`, so provider/fetch order
//! never reaches the ranking).
//!
//! Fetch policy (design §4.2 step 5): FTS and trigram run sequentially by
//! default; `ProviderFetch::Concurrent` joins both providers and is
//! config-gated, off by default. The log always depends on the ranking
//! result and runs after it, on the caller's path.
//!
//! Provider failure is structural (`EngineError::ProviderFailed`): the
//! search aborts with a hard error — never a ranking computed from a
//! partial candidate set (search-engine delta, OPT-08).

use search::engine::{CandidateProvider, EngineError, SearchEngine};
use search::types::SearchOutcome;
use uuid::Uuid;

/// FTS/trigram fetch policy (design §4.2): sequential is the default;
/// the concurrent variant must be enabled by configuration only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFetch {
    /// Providers run one after the other (default).
    Sequential,
    /// Both providers are joined with `tokio::join!` — only for measured
    /// benefit with real pool capacity (OPT-10).
    Concurrent,
}

/// Runs one orchestrated search: normalize, await both async providers
/// (fetch policy per configuration), and score the explicit candidates.
/// A provider failure propagates as the structural `ProviderFailed` error;
/// the outcome is identical for both fetch policies because the canonical
/// candidate ordering happens inside `score`.
pub async fn run_search<F: CandidateProvider, T: CandidateProvider>(
    engine: &SearchEngine,
    generation_id: Uuid,
    query: &str,
    fts: &F,
    trigram: &T,
    fetch: ProviderFetch,
) -> Result<SearchOutcome, EngineError> {
    let normalized = engine.normalize(query);

    let candidates = match fetch {
        ProviderFetch::Sequential => {
            let mut candidates = fts.candidates(generation_id, &normalized).await?;
            candidates.extend(trigram.candidates(generation_id, &normalized).await?);
            candidates
        }
        ProviderFetch::Concurrent => {
            let (fts_candidates, trigram_candidates) = tokio::join!(
                fts.candidates(generation_id, &normalized),
                trigram.candidates(generation_id, &normalized),
            );
            let mut candidates = fts_candidates?;
            candidates.extend(trigram_candidates?);
            candidates
        }
    };

    Ok(engine.score(&normalized, candidates))
}
