//! DB-backed `CandidateProvider` implementations (SE-7, task 78): the
//! PostgreSQL FTS provider (`FTS_TEXT`, over `life_events.generated_tsvector`)
//! and the pg_trgm similarity provider (`TRIGRAM`, over name+keywords).
//! Implementations live in `crates/db` per the design dependency arrow
//! `db → search`; the pure engine only sees the trait seam.
//!
//! Since S4b (task 11) the providers are async implementations invoked
//! directly from the async orchestration layer: the synchronous bridge and
//! its shared runtime are gone from the search path.

pub mod fts;
pub mod generation_trigram;
pub mod orchestrator;
pub mod trigram;

use search::engine::EngineError;
use uuid::Uuid;

/// Stage-2 placeholder generation id for the legacy (not yet
/// generation-scoped) tables: the async provider contract already carries
/// the request's generation scope, but generation-scoped projections land
/// in stage 3 (S5–S7); until then the legacy queries ignore the id. The
/// stage-3 snapshot replaces this with the request's captured generation.
pub const LEGACY_GENERATION_ID: Uuid = Uuid::nil();

/// Builds the provider-side query text: the canonical token forms joined
/// with spaces — the same de-accented lowercase alphabet the engine matches
/// keywords in (`coche` arrives as `vehiculo`, so the DB surfaces match the
/// canonical term too).
pub(crate) fn canonical_query_text(query: &search::types::NormalizedQuery) -> String {
    query
        .tokens
        .iter()
        .map(|token| token.canonical.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Maps a sqlx failure into the engine's typed provider error (design §3:
/// a failing provider is a structural hard error, never silently dropped).
pub(crate) fn provider_failed(rule_name: &'static str, err: sqlx::Error) -> EngineError {
    EngineError::ProviderFailed {
        rule_name,
        message: err.to_string(),
    }
}
