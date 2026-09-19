//! Engine facade (SE-7, task 18): one pure call composing normalize →
//! tokenize → match → rules → rank → confidence → selection. Candidate
//! generation sits behind the `CandidateProvider` trait so the ranker never
//! changes when a new provider (FTS, trigram, a future one) appears; the
//! embedding seam stays empty by contract (task 19).
//!
//! Since S4b (task 10, search-engine delta MODIFIED) the provider seam is
//! asynchronous and generation-scoped: `candidates` returns a future the
//! caller awaits from its own async context, and every invocation carries
//! the request's captured `generation_id`. The trait path carries no
//! database, HTTP, or runtime dependency — awaiting happens outside this
//! crate (the db-side orchestrator, task 11) and the engine stays pure and
//! deterministic.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use thiserror::Error;
use uuid::Uuid;

use crate::confidence::confidence;
use crate::matcher::match_keywords;
use crate::ranker::rank;
use crate::rules::action_entity_entries;
use crate::selection::select;
use crate::tokenizer::{SynonymMap, tokenize};
use crate::types::{Candidate, NormalizedQuery};
use crate::types::{EventLexicon, EventScore, SearchOutcome};

/// Typed engine error (design §3 error strategy): a failing provider is a
/// structural problem, so it propagates as a hard error instead of being
/// silently dropped.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("candidate provider `{rule_name}` failed: {message}")]
    ProviderFailed {
        rule_name: &'static str,
        message: String,
    },
}

/// The future an async provider's `candidates` call resolves into (S4b
/// task 10): boxed to keep the trait dyn-compatible, so the orchestrator
/// and the test harness can hold heterogeneous providers as trait objects
/// and a future embedding provider can still slot in behind the same seam
/// without touching the ranker.
pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Candidate>, EngineError>> + Send + 'a>>;

/// Source of provider-sourced candidates (SE-7). Implementations live
/// outside the pure crate (e.g. `crates/db`'s FTS and trigram providers);
/// each contribution is reported under the provider's own rule name. The
/// seam carries no model or vector-store types: the embedding variant is
/// deliberately absent in the MVP.
pub trait CandidateProvider: Send + Sync {
    /// Rule name this provider's contributions are reported under
    /// (`FTS_TEXT`, `TRIGRAM`) — preserved verbatim through the async
    /// contract (S4b task 10).
    fn rule_name(&self) -> &'static str;

    /// Produces the provider's per-event score contributions for the
    /// already-normalized query, scoped to the request's captured
    /// `generation_id`. The returned future is awaited by the caller's own
    /// async context (the orchestrator awaits providers directly on the
    /// HTTP path); the engine never blocks a thread on database I/O.
    fn candidates<'a>(
        &'a self,
        generation_id: Uuid,
        query: &'a NormalizedQuery,
    ) -> ProviderFuture<'a>;
}

/// The pure deterministic search engine over a taxonomy lexicon.
pub struct SearchEngine {
    events: Vec<EventLexicon>,
    synonyms: SynonymMap,
}

impl SearchEngine {
    /// Builds an engine from the taxonomy-fed scoring lexicons (the YAML
    /// events' in-memory projection) and the synonym map.
    pub fn new(events: Vec<EventLexicon>, synonyms: SynonymMap) -> Self {
        SearchEngine { events, synonyms }
    }

    /// Runs the full deterministic pipeline over `query` (SE-7, task 18):
    /// normalize → collect the async providers' candidates → score.
    /// Provider-list order is irrelevant to the output: candidates are
    /// sorted into a canonical order before ranking (the sort lives inside
    /// `score`). Awaiting stays with the caller; this crate has no runtime
    /// dependency. Production serving goes through the db-side async
    /// orchestrator (S4b task 11); this composition remains the stub/test
    /// path (design §4.2 step 3).
    pub async fn search(
        &self,
        generation_id: Uuid,
        query: &str,
        providers: &[&dyn CandidateProvider],
    ) -> Result<SearchOutcome, EngineError> {
        let normalized = self.normalize(query);

        let mut candidates = Vec::new();
        for provider in providers {
            candidates.extend(provider.candidates(generation_id, &normalized).await?);
        }
        Ok(self.score(&normalized, candidates))
    }

    /// The engine's normalization step, exposed for the async orchestration
    /// layer (S4b task 11): the orchestrator normalizes, awaits the
    /// providers, and hands explicit candidates back to `score` — the same
    /// tokenization `search` applies, so both paths see identical inputs.
    pub fn normalize(&self, query: &str) -> NormalizedQuery {
        tokenize(query, &self.synonyms)
    }

    /// Scores an already-normalized query against explicitly provided
    /// candidates (S4a task 9, OPT-08): the pure ranking boundary that the
    /// async orchestration layer (design §4.2) will call after fetching
    /// candidates. Composes the existing match + rules + canonical
    /// candidate ordering + rank + confidence + selection steps with no
    /// provider, database, HTTP, or runtime dependency; identical inputs
    /// produce identical outcomes regardless of candidate input order.
    pub fn score(&self, normalized: &NormalizedQuery, candidates: Vec<Candidate>) -> SearchOutcome {
        let event_scores: Vec<EventScore> = self
            .events
            .iter()
            .map(|lexicon| {
                let mut entries = match_keywords(normalized, &lexicon.keywords);
                entries.extend(action_entity_entries(normalized, &lexicon.rules));
                EventScore {
                    slug: lexicon.slug.clone(),
                    entries,
                }
            })
            .collect();

        let mut candidates = candidates;
        candidates.sort_by(|a, b| {
            (&a.event_slug, &a.rule_name, a.value).cmp(&(&b.event_slug, &b.rule_name, b.value))
        });

        let results = rank(normalized, &event_scores, &candidates);
        let scores: Vec<i64> = results.iter().map(|result| result.score).collect();
        let confidence = confidence(&scores);
        let categories = self.categories();
        let selection = select(confidence, &results, &categories);

        SearchOutcome {
            query: normalized.clone(),
            results,
            confidence,
            selection,
        }
    }

    /// The available category slugs, sorted and deduplicated, derived from
    /// the taxonomy events (SE-10's categories payload).
    fn categories(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|lexicon| lexicon.category.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
