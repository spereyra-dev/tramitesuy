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

/// Collects the stub providers' candidates for a query — the explicit
/// candidate input `score()` consumes (S4a task 9), awaited through the
/// async provider contract (S4b task 10).
fn stub_candidates(
    providers: &[&dyn CandidateProvider],
    normalized: &NormalizedQuery,
) -> Vec<Candidate> {
    providers
        .iter()
        .flat_map(|provider| {
            support::block_on(provider.candidates(support::STUB_GENERATION, normalized))
                .expect("stub providers must not fail")
        })
        .collect()
}

/// The S4a equivalence clause: for every selection band (open,
/// disambiguation, categories), calling `score()` with the explicitly
/// collected candidates must produce a byte-identical outcome to the full
/// `search()` pipeline with the same stub providers.
#[test]
fn score_matches_search_for_open_disambiguation_and_categories() {
    let fixture = vehiculos_fixture();
    let engine = SearchEngine::new(
        fixture.events.iter().map(support::event_lexicon).collect(),
        fixture.synonyms.clone(),
    );

    // Open: a dominant winner whose confidence clears the open band.
    let query = "compre un auto usado";
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
    };
    let normalized = search::tokenizer::tokenize(query, &fixture.synonyms);
    let candidates = stub_candidates(&[&fts, &trigram], &normalized);

    let via_search =
        support::block_on(engine.search(support::STUB_GENERATION, query, &[&fts, &trigram]))
            .expect("search must succeed");
    let via_score = engine.score(&normalized, candidates);

    assert_eq!(
        via_search, via_score,
        "score() must reproduce search() byte for byte in the open band"
    );
    assert_eq!(via_score.selection.mode, search::types::SelectionMode::Open);

    // Disambiguation: both events tie, so confidence lands between the
    // disambiguation and open thresholds.
    let query = "vehiculo";
    let fts = StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 2)],
    };
    let trigram = StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
    };
    let normalized = search::tokenizer::tokenize(query, &fixture.synonyms);
    let candidates = stub_candidates(&[&fts, &trigram], &normalized);

    let via_search =
        support::block_on(engine.search(support::STUB_GENERATION, query, &[&fts, &trigram]))
            .expect("search must succeed");
    let via_score = engine.score(&normalized, candidates);

    assert_eq!(
        via_search, via_score,
        "score() must reproduce search() byte for byte in the disambiguation band"
    );
    assert_eq!(
        via_score.selection.mode,
        search::types::SelectionMode::Disambiguation
    );

    // Categories: a zero-match query takes the no-result path.
    let query = "xyzzy qwertyjf";
    let normalized = search::tokenizer::tokenize(query, &fixture.synonyms);
    let candidates = stub_candidates(&[], &normalized);

    let via_search = support::block_on(engine.search(support::STUB_GENERATION, query, &[]))
        .expect("search must succeed");
    let via_score = engine.score(&normalized, candidates);

    assert_eq!(
        via_search, via_score,
        "score() must reproduce search() byte for byte in the categories band"
    );
    assert_eq!(
        via_score.selection.mode,
        search::types::SelectionMode::Categories
    );
}

#[test]
fn search_opens_a_dominant_winner_end_to_end() {
    let engine = fixture_engine();
    let outcome =
        support::block_on(engine.search(support::STUB_GENERATION, "compre un auto usado", &[]))
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

    let outcome = support::block_on(engine.search(
        support::STUB_GENERATION,
        "compre un auto usado",
        &[&fts, &trigram],
    ))
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

    let forward = support::block_on(engine.search(
        support::STUB_GENERATION,
        "compre un auto usado",
        &[&fts, &trigram],
    ))
    .expect("search must succeed");
    let reversed = support::block_on(engine.search(
        support::STUB_GENERATION,
        "compre un auto usado",
        &[&trigram, &fts],
    ))
    .expect("search must succeed");

    assert_eq!(
        forward, reversed,
        "provider order is an implementation detail, never an output"
    );
}

#[test]
fn a_zero_match_query_takes_the_categories_path() {
    let engine = fixture_engine();
    let outcome = support::block_on(engine.search(support::STUB_GENERATION, "xyzzy qwertyjf", &[]))
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

    let comprar = support::block_on(engine.search(support::STUB_GENERATION, "compre un auto", &[]))
        .expect("search must succeed");
    assert_eq!(
        comprar.selection.event_slug.as_deref(),
        Some("comprar-vehiculo")
    );

    let vender = support::block_on(engine.search(support::STUB_GENERATION, "vendi mi auto", &[]))
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

    fn candidates<'a>(
        &'a self,
        _generation_id: uuid::Uuid,
        _query: &'a NormalizedQuery,
    ) -> search::engine::ProviderFuture<'a> {
        Box::pin(async {
            Err(EngineError::ProviderFailed {
                rule_name: "FTS_TEXT",
                message: "boom".to_string(),
            })
        })
    }
}

/// S4a regression: the canonical candidate ordering lives before scoring,
/// so the outcome is invariant under the order the candidates arrive in.
#[test]
fn score_is_invariant_under_candidate_input_order() {
    let fixture = vehiculos_fixture();
    let engine = SearchEngine::new(
        fixture.events.iter().map(support::event_lexicon).collect(),
        fixture.synonyms.clone(),
    );
    let normalized = search::tokenizer::tokenize("compre un auto usado", &fixture.synonyms);

    // The same candidate multiset in shuffled and canonically sorted form.
    let shuffled = vec![
        Candidate {
            event_slug: "vender-vehiculo".to_string(),
            rule_name: "TRIGRAM".to_string(),
            value: 2,
        },
        Candidate {
            event_slug: "comprar-vehiculo".to_string(),
            rule_name: "FTS_TEXT".to_string(),
            value: 5,
        },
        Candidate {
            event_slug: "comprar-vehiculo".to_string(),
            rule_name: "TRIGRAM".to_string(),
            value: 3,
        },
        Candidate {
            event_slug: "vender-vehiculo".to_string(),
            rule_name: "FTS_TEXT".to_string(),
            value: 1,
        },
    ];
    let mut canonical = shuffled.clone();
    canonical.sort_by(|a, b| {
        (&a.event_slug, &a.rule_name, a.value).cmp(&(&b.event_slug, &b.rule_name, b.value))
    });
    assert_ne!(shuffled, canonical, "fixture must start unordered");

    assert_eq!(
        engine.score(&normalized, shuffled),
        engine.score(&normalized, canonical),
        "candidate ordering must be canonicalized before scoring"
    );
}

#[test]
fn a_provider_failure_is_a_hard_error() {
    let engine = fixture_engine();
    let result = support::block_on(engine.search(
        support::STUB_GENERATION,
        "compre un auto usado",
        &[&FailingProvider],
    ));
    assert!(
        result.is_err(),
        "a structural provider failure must propagate, not be swallowed"
    );
}

/// A provider whose captured generation id is recorded: the async seam
/// carries the request's generation scope (S4b task 10).
#[derive(Debug)]
struct GenerationRecordingProvider {
    name: &'static str,
    contributions: Vec<(&'static str, i64)>,
    received: std::sync::Arc<std::sync::Mutex<Vec<uuid::Uuid>>>,
}

impl CandidateProvider for GenerationRecordingProvider {
    fn rule_name(&self) -> &'static str {
        self.name
    }

    fn candidates<'a>(
        &'a self,
        generation_id: uuid::Uuid,
        _query: &'a NormalizedQuery,
    ) -> search::engine::ProviderFuture<'a> {
        Box::pin(async move {
            self.received.lock().unwrap().push(generation_id);
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

/// S4b task 10 RED: the async provider seam must carry the request's
/// captured `generation_id` to every provider invocation.
#[test]
fn async_providers_receive_the_request_generation_id() {
    let engine = fixture_engine();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let fts = GenerationRecordingProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
        received: captured.clone(),
    };
    let trigram = GenerationRecordingProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
        received: captured.clone(),
    };
    // A fixed non-nil id: proves the engine forwards the caller's captured
    // value (any distinct UUID would do; nil is the stub placeholder).
    let generation = uuid::Uuid::from_u128(0xA1B2_C3D4_E5F6_0718_293A_4B5C_6D7E_8F90);

    let outcome =
        support::block_on(engine.search(generation, "compre un auto usado", &[&fts, &trigram]))
            .expect("search must succeed");

    assert_eq!(
        *captured.lock().unwrap(),
        vec![generation, generation],
        "every provider must receive exactly the generation id the caller captured"
    );
    assert_eq!(outcome.results[0].slug, "comprar-vehiculo");
}

/// S4b task 10 RED: async stub providers returning generation-scoped
/// candidates still yield both `FTS_TEXT` and `TRIGRAM` explanation entries
/// whose values sum exactly to the final score (the MODIFIED provider-trait
/// contract preserves the identified contributions verbatim).
#[test]
fn async_stub_providers_keep_both_named_contributions_summing_to_the_score() {
    let engine = fixture_engine();
    let fts = GenerationRecordingProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5)],
        received: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let trigram = GenerationRecordingProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2)],
        received: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    let outcome = support::block_on(engine.search(
        support::STUB_GENERATION,
        "compre un auto usado",
        &[&fts, &trigram],
    ))
    .expect("search must succeed");

    let top = &outcome.results[0];
    assert_eq!(top.slug, "comprar-vehiculo");
    assert_eq!(top.score, 41, "36 taxonomy + 5 FTS_TEXT");
    let fts_entries: Vec<i64> = top
        .explanation
        .entries
        .iter()
        .filter(|entry| entry.rule_name == "FTS_TEXT")
        .map(|entry| entry.value)
        .collect();
    assert_eq!(
        fts_entries,
        vec![5],
        "the FTS_TEXT contribution must appear verbatim under its own rule name"
    );
    let trigram_top = &outcome.results[1];
    assert_eq!(trigram_top.slug, "vender-vehiculo");
    assert_eq!(trigram_top.score, 10, "8 taxonomy + 2 TRIGRAM");
    let trigram_entries: Vec<i64> = trigram_top
        .explanation
        .entries
        .iter()
        .filter(|entry| entry.rule_name == "TRIGRAM")
        .map(|entry| entry.value)
        .collect();
    assert_eq!(
        trigram_entries,
        vec![2],
        "the TRIGRAM contribution must appear verbatim under its own rule name"
    );
    // Every result's explanation reconstructs its score exactly.
    for result in &outcome.results {
        let sum: i64 = result
            .explanation
            .entries
            .iter()
            .map(|entry| entry.value)
            .sum();
        assert_eq!(
            sum, result.score,
            "explanation entries must sum to the final score for {}",
            result.slug
        );
    }
}
