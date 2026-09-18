//! Engine facade (SE-7, task 18): one pure call composing normalize →
//! tokenize → match → rules → rank → confidence → selection. Candidate
//! generation sits behind the `CandidateProvider` trait so the ranker never
//! changes when a new provider (FTS, trigram, a future one) appears; the
//! embedding seam stays empty by contract (task 19).

use std::collections::BTreeSet;

use thiserror::Error;

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

/// Source of provider-sourced candidates (SE-7). Implementations live
/// outside the pure crate (e.g. `crates/db`'s FTS and trigram providers);
/// each contribution is reported under the provider's own rule name. The
/// seam carries no model or vector-store types: the embedding variant is
/// deliberately absent in the MVP.
pub trait CandidateProvider {
    /// Rule name this provider's contributions are reported under
    /// (`FTS_TEXT`, `TRIGRAM`).
    fn rule_name(&self) -> &'static str;

    /// Produces the provider's per-event score contributions for the
    /// already-normalized query.
    fn candidates(&self, query: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError>;
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

    /// Runs the full deterministic pipeline over `query` (SE-7, task 18).
    /// Provider-list order is irrelevant to the output: candidates are
    /// sorted into a canonical order before ranking.
    pub fn search(
        &self,
        query: &str,
        providers: &[&dyn CandidateProvider],
    ) -> Result<SearchOutcome, EngineError> {
        let normalized = tokenize(query, &self.synonyms);

        let event_scores: Vec<EventScore> = self
            .events
            .iter()
            .map(|lexicon| {
                let mut entries = match_keywords(&normalized, &lexicon.keywords);
                entries.extend(action_entity_entries(&normalized, &lexicon.rules));
                EventScore {
                    slug: lexicon.slug.clone(),
                    entries,
                }
            })
            .collect();

        let mut candidates = Vec::new();
        for provider in providers {
            candidates.extend(provider.candidates(&normalized)?);
        }
        candidates.sort_by(|a, b| {
            (&a.event_slug, &a.rule_name, a.value).cmp(&(&b.event_slug, &b.rule_name, b.value))
        });

        let results = rank(&normalized, &event_scores, &candidates);
        let scores: Vec<i64> = results.iter().map(|result| result.score).collect();
        let confidence = confidence(&scores);
        let categories = self.categories();
        let selection = select(confidence, &results, &categories);

        Ok(SearchOutcome {
            query: normalized,
            results,
            confidence,
            selection,
        })
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
