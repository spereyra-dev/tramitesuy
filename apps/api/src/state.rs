//! Shared handler state (design §2 `AppState`, task 84): the search engine,
//! the YAML-loaded taxonomy, and the database pool. The taxonomy is loaded
//! from `data/events/*.yaml` (plus categories/synonyms) at boot and cached
//! here — the YAML remains the ranker's single source of truth (design §4.2,
//! TX-1); the DB `life_events`/`life_event_keywords` tables are projections
//! consumed by the FTS/trigram providers and the website, never by the
//! ranker. That keeps `/search/debug` reconstruction exact.

use std::path::Path;
use std::sync::Arc;

use crate::metrics::Metrics;
use search::engine::SearchEngine;
use search::tokenizer::SynonymMap;
use search::types::{
    CombinationRule as EngineRule, EventLexicon, Keyword as EngineKeyword,
    KeywordKind as EngineKeywordKind,
};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    /// The deterministic search engine, built once at boot from the YAML
    /// taxonomy (never per request, never from the DB projection).
    pub engine: Arc<SearchEngine>,
    /// The loaded YAML taxonomy: the ranker's source of truth and the
    /// resolver for event/category display names.
    pub taxonomy: Arc<taxonomy::model::Taxonomy>,
    pub pool: PgPool,
    /// The privacy-safe metrics sink (task 1): every served request reports
    /// route/status latency, SQL ops, cache events, and generation state
    /// through this seam — never query-derived text (R14).
    pub metrics: Arc<dyn Metrics>,
}

impl AppState {
    /// Boots the state: loads the taxonomy directory and builds the engine
    /// once. `data_dir` must contain the `events/`, `categories/`, and
    /// `synonyms/` subdirectories (the `taxonomy-validate` CLI owns
    /// validation in CI; boot requires only a loadable taxonomy).
    pub fn build(pool: PgPool, data_dir: &Path) -> Result<Self, String> {
        Self::build_with_metrics(
            pool,
            data_dir,
            Arc::new(crate::metrics::MemoryMetrics::new()),
        )
    }

    /// Boots the state with an injected metrics sink (task 1 seam): the
    /// boot path is identical, but tests can read the counters.
    pub fn build_with_metrics(
        pool: PgPool,
        data_dir: &Path,
        metrics: Arc<dyn Metrics>,
    ) -> Result<Self, String> {
        let taxonomy = taxonomy::loader::load_data_dir(data_dir)
            .map_err(|error| format!("taxonomy load failed: {error}"))?;
        let synonyms: SynonymMap = taxonomy
            .synonyms
            .iter()
            .map(|source| {
                (
                    source.synonym.term.clone(),
                    source.synonym.canonical.clone(),
                )
            })
            .collect();
        let events: Vec<EventLexicon> = taxonomy
            .events
            .iter()
            .map(|source| event_lexicon(&source.event))
            .collect();
        Ok(AppState {
            engine: Arc::new(SearchEngine::new(events, synonyms)),
            taxonomy: Arc::new(taxonomy),
            pool,
            metrics,
        })
    }

    /// The YAML display name of an event slug.
    pub fn event_name(&self, slug: &str) -> Option<&str> {
        self.taxonomy
            .events
            .iter()
            .find(|source| source.event.slug == slug)
            .map(|source| source.event.name.as_str())
    }

    /// The YAML display name of a category slug.
    pub fn category_name(&self, slug: &str) -> Option<&str> {
        self.taxonomy
            .categories
            .iter()
            .find(|source| source.category.slug == slug)
            .map(|source| source.category.name.as_str())
    }
}

/// Projects one taxonomy event into the engine-side scoring lexicon. The
/// mapping is shape-only (terms, kinds, weights, rules) — no seed domain
/// data lives in code (TX-1).
fn event_lexicon(event: &taxonomy::model::Event) -> EventLexicon {
    EventLexicon {
        slug: event.slug.clone(),
        category: event.category.clone(),
        keywords: event
            .keywords
            .iter()
            .map(|keyword| EngineKeyword {
                term: keyword.term.clone(),
                canonical: keyword.canonical_or_term().to_string(),
                kind: match keyword.keyword_type {
                    taxonomy::model::KeywordType::Action => EngineKeywordKind::Action,
                    taxonomy::model::KeywordType::Entity => EngineKeywordKind::Entity,
                    taxonomy::model::KeywordType::Modifier => EngineKeywordKind::Modifier,
                    taxonomy::model::KeywordType::Context => EngineKeywordKind::Context,
                },
                weight: keyword.weight,
                negative: keyword.negative,
            })
            .collect(),
        rules: event
            .rules
            .iter()
            .map(|rule| EngineRule {
                action: rule.action.clone(),
                entity: rule.entity.clone(),
                bonus: rule.bonus,
            })
            .collect(),
    }
}
