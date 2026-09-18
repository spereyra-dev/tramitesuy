//! Engine facade contract (SE-7, SE-9, SE-10, task 18): one pure call
//! composing normalize → tokenize → match → rules → rank → confidence →
//! selection, with candidates arriving through the `CandidateProvider`
//! trait and the outcome invariant under provider-list permutation.

mod support;

use search::engine::{CandidateProvider, EngineError, SearchEngine};
use search::types::{Candidate, NormalizedQuery};
use support::{StubProvider, vehiculos_fixture};

fn fixture_engine() -> SearchEngine {
    let fixture = vehiculos_fixture();
    SearchEngine::new(
        fixture.events.iter().map(support::event_lexicon).collect(),
        fixture.synonyms,
    )
}

#[test]
fn search_opens_a_dominant_winner_end_to_end() {
    let engine = fixture_engine();
    let outcome = engine
        .search("compre un auto usado", &[])
        .expect("search must succeed");

    assert_eq!(
        outcome.results.len(),
        2,
        "vender-vehiculo also matches vehiculo"
    );
    assert_eq!(outcome.results[0].slug, "comprar-vehiculo");
    assert_eq!(
        outcome.results[0].score, 36,
        "10 + 8 + 3 + ACTION_ENTITY 15"
    );
    assert_eq!(
        outcome.confidence, 0.82,
        "36 / (36 + 8) = 0.8181... rounds to 0.82"
    );
    assert_eq!(outcome.selection.mode, search::types::SelectionMode::Open);
    assert_eq!(
        outcome.selection.event_slug.as_deref(),
        Some("comprar-vehiculo")
    );
    // The explanation reconstructs the score exactly (SE-11 through the facade).
    let sum: i64 = outcome.results[0]
        .explanation
        .entries
        .iter()
        .map(|e| e.value)
        .sum();
    assert_eq!(sum, outcome.results[0].score);
}

#[test]
fn provider_contributions_merge_under_their_own_rule_name() {
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
    };

    let outcome = engine
        .search("compre un auto usado", &[&fts, &trigram])
        .expect("search must succeed");

    assert_eq!(outcome.results[0].slug, "comprar-vehiculo");
    assert_eq!(outcome.results[0].score, 41, "36 taxonomy + 5 FTS_TEXT");
    assert!(
        outcome.results[0]
            .explanation
            .entries
            .iter()
            .any(|entry| { entry.rule_name == "FTS_TEXT" && entry.value == 5 })
    );
    let vender = &outcome.results[1];
    assert_eq!(vender.score, 10, "8 taxonomy + 2 TRIGRAM");
    assert!(
        vender.explanation.entries.iter().any(|entry| {
            entry.rule_name == "TRIGRAM" && entry.value == 2 && entry.term.is_none()
        })
    );
}

#[test]
fn permuting_the_provider_list_does_not_change_the_outcome() {
    let engine = fixture_engine();
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5), ("vender-vehiculo", 1)],
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2), ("comprar-vehiculo", 3)],
    };

    let forward = engine
        .search("compre un auto usado", &[&fts, &trigram])
        .expect("search must succeed");
    let reversed = engine
        .search("compre un auto usado", &[&trigram, &fts])
        .expect("search must succeed");

    assert_eq!(
        forward, reversed,
        "provider order is an implementation detail, never an output"
    );
}

#[test]
fn a_zero_match_query_takes_the_categories_path() {
    let engine = fixture_engine();
    let outcome = engine
        .search("xyzzy qwertyjf", &[])
        .expect("search must succeed");

    assert!(outcome.results.is_empty());
    assert_eq!(outcome.confidence, 0.0);
    assert_eq!(
        outcome.selection.mode,
        search::types::SelectionMode::Categories
    );
    assert_eq!(
        outcome.selection.categories,
        vec!["vehiculos".to_string()],
        "the engine derives the available categories from the taxonomy events"
    );
}

#[test]
fn near_duplicate_actions_are_separable_through_the_facade() {
    let engine = fixture_engine();

    let comprar = engine
        .search("compre un auto", &[])
        .expect("search must succeed");
    assert_eq!(
        comprar.selection.event_slug.as_deref(),
        Some("comprar-vehiculo")
    );

    let vender = engine
        .search("vendi mi auto", &[])
        .expect("search must succeed");
    assert_eq!(vender.results[0].slug, "vender-vehiculo");
    assert_eq!(vender.results[0].score, 33, "10 + 8 + ACTION_ENTITY 15");
    // The negative penalty keeps comprar-vehiculo reconstructible in the list.
    let penalized = &vender.results[1];
    assert_eq!(penalized.slug, "comprar-vehiculo");
    assert_eq!(penalized.score, -7, "8 - 15");
    assert!(
        penalized
            .explanation
            .entries
            .iter()
            .any(|entry| entry.rule_name == "NEGATIVE_KEYWORD" && entry.value == -15)
    );
}

#[derive(Debug)]
struct FailingProvider;

impl CandidateProvider for FailingProvider {
    fn rule_name(&self) -> &'static str {
        "FTS_TEXT"
    }

    fn candidates(&self, _query: &NormalizedQuery) -> Result<Vec<Candidate>, EngineError> {
        Err(EngineError::ProviderFailed {
            rule_name: "FTS_TEXT",
            message: "boom".to_string(),
        })
    }
}

#[test]
fn a_provider_failure_is_a_hard_error() {
    let engine = fixture_engine();
    let result = engine.search("compre un auto usado", &[&FailingProvider]);
    assert!(
        result.is_err(),
        "a structural provider failure must propagate, not be swallowed"
    );
}
