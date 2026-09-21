//! Task 11 (S4b, design §4.2): the async orchestration layer. `run_search`
//! normalizes, awaits the async providers (sequential by default; a
//! concurrent variant is config-gated and off by default), and hands
//! explicit candidates to the pure `score` boundary — canonical ordering
//! happens there, so provider/fetch order never reaches the ranking.

use std::collections::HashMap;

use db::providers::orchestrator::{ProviderFetch, run_search};
use search::engine::{CandidateProvider, EngineError, ProviderFuture, SearchEngine};
use search::types::{
    Candidate, CombinationRule, EventLexicon, Keyword, KeywordKind, NormalizedQuery,
};
use uuid::Uuid;

/// The generation placeholder: the orchestrator forwards whatever id the
/// caller captured (the legacy path uses `LEGACY_GENERATION_ID` until the
/// stage-3 snapshot lands).
const GENERATION: Uuid = Uuid::nil();

struct StubProvider {
    name: &'static str,
    contributions: Vec<(&'static str, i64)>,
    fail: bool,
}

impl CandidateProvider for StubProvider {
    fn rule_name(&self) -> &'static str {
        self.name
    }

    fn candidates<'a>(
        &'a self,
        _generation_id: Uuid,
        _query: &'a NormalizedQuery,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if self.fail {
                return Err(EngineError::ProviderFailed {
                    rule_name: self.name,
                    message: "boom".to_string(),
                });
            }
            Ok(self
                .contributions
                .iter()
                .map(|(slug, value)| Candidate {
                    event_slug: slug.to_string(),
                    rule_name: self.name.to_string(),
                    value: *value,
                })
                .collect())
        })
    }
}

fn keyword(term: &str, kind: KeywordKind, weight: i64) -> Keyword {
    Keyword {
        term: term.to_string(),
        canonical: term.to_string(),
        kind,
        weight,
        negative: false,
    }
}

fn fixture_engine() -> SearchEngine {
    let synonyms: HashMap<String, String> = [("auto".to_string(), "vehiculo".to_string())]
        .into_iter()
        .collect();
    let comprar = EventLexicon {
        slug: "comprar-vehiculo".to_string(),
        category: "vehiculos".to_string(),
        keywords: vec![
            keyword("comprar", KeywordKind::Action, 10),
            keyword("vehiculo", KeywordKind::Entity, 8),
            keyword("usado", KeywordKind::Modifier, 3),
        ],
        rules: vec![CombinationRule {
            action: "comprar".to_string(),
            entity: "vehiculo".to_string(),
            bonus: 15,
        }],
    };
    let vender = EventLexicon {
        slug: "vender-vehiculo".to_string(),
        category: "vehiculos".to_string(),
        keywords: vec![
            keyword("vender", KeywordKind::Action, 10),
            keyword("vehiculo", KeywordKind::Entity, 8),
        ],
        rules: vec![CombinationRule {
            action: "vender".to_string(),
            entity: "vehiculo".to_string(),
            bonus: 15,
        }],
    };
    SearchEngine::new(vec![comprar, vender], synonyms)
}

#[tokio::test(flavor = "multi_thread")]
async fn run_search_composes_normalize_providers_and_score() {
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
        fail: false,
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
        fail: false,
    };

    let outcome = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Sequential,
    )
    .await
    .expect("orchestrated search succeeds");

    // The outcome equals the manual composition: normalize → provider
    // candidates → score (the same boundary search() and the API use).
    let normalized = engine.normalize("compre un auto usado");
    let mut candidates = fts
        .candidates(GENERATION, &normalized)
        .await
        .expect("stub fts succeeds");
    candidates.extend(
        trigram
            .candidates(GENERATION, &normalized)
            .await
            .expect("stub trigram succeeds"),
    );
    assert_eq!(outcome, engine.score(&normalized, candidates));

    assert_eq!(outcome.selection.mode, search::types::SelectionMode::Open);
    assert_eq!(
        outcome.selection.event_slug.as_deref(),
        Some("comprar-vehiculo")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_fetch_yields_the_same_outcome_as_sequential() {
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5), ("vender-vehiculo", 1)],
        fail: false,
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2), ("comprar-vehiculo", 3)],
        fail: false,
    };

    let sequential = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Sequential,
    )
    .await
    .expect("sequential search succeeds");
    let concurrent = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Concurrent,
    )
    .await
    .expect("concurrent search succeeds");

    assert_eq!(
        sequential, concurrent,
        "the fetch policy must never change the ranking: candidates are \
         canonically ordered before scoring"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_failure_is_a_structural_error_never_a_partial_ranking() {
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
        fail: false,
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![],
        fail: true,
    };

    let sequential = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Sequential,
    )
    .await;
    assert!(
        sequential.is_err(),
        "a failing provider aborts the search, never silently ranks a partial set"
    );

    let concurrent = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Concurrent,
    )
    .await;
    assert!(
        concurrent.is_err(),
        "the concurrent policy propagates the same structural failure"
    );
    let error = concurrent.unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::ProviderFailed {
                rule_name: "TRIGRAM",
                ..
            }
        ),
        "the structural error names the failing provider: {error:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn candidate_order_from_the_providers_is_canonicalized_before_scoring() {
    // The trigram provider deliberately yields its candidates in reverse
    // order; the orchestrator passes provider output through unchanged and
    // `score` canonicalizes — identical outcomes both ways.
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5), ("vender-vehiculo", 1)],
        fail: false,
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2), ("comprar-vehiculo", 3)],
        fail: false,
    };

    let outcome = run_search(
        &engine,
        GENERATION,
        "compre un auto usado",
        &fts,
        &trigram,
        ProviderFetch::Sequential,
    )
    .await
    .expect("search succeeds");

    assert_eq!(
        outcome.results.first().map(|r| r.slug.as_str()),
        Some("comprar-vehiculo"),
        "the top result must be independent of provider yield order"
    );
}
