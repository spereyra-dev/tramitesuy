//! DB-backed `CandidateProvider` implementations (SE-7, task 78): the
//! PostgreSQL FTS provider (`FTS_TEXT`, over `life_events.generated_tsvector`)
//! and the pg_trgm similarity provider (`TRIGRAM`, over name+keywords).
//! Implementations live in `crates/db` per the design dependency arrow
//! `db → search`; the pure engine only sees the trait seam.

pub mod fts;
pub mod trigram;

use search::engine::EngineError;
use std::sync::OnceLock;

/// Bridges the synchronous `CandidateProvider` trait into sqlx's async
/// queries — the same adapter pattern as
/// `repos::procedures::PostgresProcedureRepository`. Called from within a
/// multi-thread tokio runtime (the axum worker, the DB-backed tests) it
/// blocks via `block_in_place`; synchronous callers outside any runtime fall
/// back to a shared internal runtime. Current-thread runtimes cannot host
/// the bridge (tokio forbids blocking there); neither the API server nor the
/// tests use one.
pub(crate) fn bridge_block_on<F: std::future::Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => shared_runtime().block_on(fut),
    }
}

/// Shared runtime for synchronous callers outside any tokio context.
fn shared_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("provider bridge runtime")
    })
}

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
