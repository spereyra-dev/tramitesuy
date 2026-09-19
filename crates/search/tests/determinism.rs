//! Determinism contract (SE-1, task 8): identical inputs produce identical
//! output on every run in the same process.
//!
//! Unit A1 covered the deterministic foundations (normalization, synonym
//! canonicalization) and unit A2 extended the assertion through the ranker.
//! Unit A3 closes the full SE-1 clause: the complete engine pipeline —
//! scores, ordering, confidence, and selection — is asserted byte-identical
//! across runs (task 18).

mod support;

use search::normalizer::normalize;
use search::tokenizer::canonicalize_tokens;
use std::collections::HashMap;

#[test]
fn repeated_runs_are_identical() {
    let fixture = support::vehiculos_fixture();

    let run = || -> search::types::NormalizedQuery {
        let normalized = normalize("¡¿Compré un AUTO usado?! Cédula 4.123.456-7");
        canonicalize_tokens(&normalized, &fixture.synonyms)
    };

    let first = run();
    let second = run();

    assert_eq!(first, second, "two identical runs must be byte-identical");
}

#[test]
fn synonym_map_lookup_order_does_not_affect_output() {
    // Feed the same rules through two differently-ordered maps to prove the
    // canonicalization output depends only on the mapping, not on HashMap
    // iteration order.
    let fixture = support::vehiculos_fixture();

    let mut forward: HashMap<String, String> = HashMap::new();
    let mut reversed: HashMap<String, String> = HashMap::new();
    let entries: Vec<(String, String)> = fixture
        .synonyms
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in &entries {
        forward.insert(k.clone(), v.clone());
    }
    for (k, v) in entries.iter().rev() {
        reversed.insert(k.clone(), v.clone());
    }

    for query in ["compre un coche usado", "automovil patente"] {
        let normalized = normalize(query);
        let a = canonicalize_tokens(&normalized, &forward);
        let b = canonicalize_tokens(&normalized, &reversed);
        assert_eq!(a, b, "token canonicalization must be order-independent");
    }
}

#[test]
fn ranked_scores_and_ordering_are_identical_across_runs() {
    // Ranker stage of the SE-1 determinism clause (task 14): identical inputs
    // produce an identical ranked list, byte for byte, in one process.
    let fixture = support::vehiculos_fixture();

    let run = || -> Vec<search::types::ScoredEvent> {
        let query = search::tokenizer::tokenize("compre un auto usado", &fixture.synonyms);
        let scores: Vec<search::types::EventScore> = fixture
            .events
            .iter()
            .map(|event| support::score_event(&query, event))
            .collect();
        search::ranker::rank(&query, &scores, &[])
    };

    assert_eq!(run(), run(), "two identical runs must be byte-identical");
}

#[test]
fn full_pipeline_outcome_is_identical_across_runs() {
    // SE-1 full clause, closed in task 18: the whole engine facade run —
    // scores, ordering, confidence, and selection — is byte-identical for
    // identical inputs, in one process.
    let fixture = support::vehiculos_fixture();
    let engine = search::engine::SearchEngine::new(
        fixture.events.iter().map(support::event_lexicon).collect(),
        fixture.synonyms,
    );

    let run = || {
        support::block_on(engine.search(support::STUB_GENERATION, "¡¿Compré un AUTO usado?!", &[]))
            .expect("search must succeed")
    };

    assert_eq!(
        run(),
        run(),
        "the full pipeline (scores, ordering, confidence, selection, explanations) must be byte-identical"
    );
}

#[test]
fn score_outcome_is_identical_across_runs_and_provider_orders() {
    // S4a task 9: the new explicit-candidate boundary keeps the SE-1
    // determinism clause — `score()` with explicitly collected candidates is
    // byte-identical to `search()` with the same stub providers, and the
    // canonical ordering before scoring makes provider-list order
    // irrelevant.
    let fixture = support::vehiculos_fixture();
    let engine = search::engine::SearchEngine::new(
        fixture.events.iter().map(support::event_lexicon).collect(),
        fixture.synonyms.clone(),
    );
    let fts = support::StubProvider {
        name: "FTS_TEXT",
        contributions: vec![("comprar-vehiculo", 5), ("vender-vehiculo", 1)],
    };
    let trigram = support::StubProvider {
        name: "TRIGRAM",
        contributions: vec![("vender-vehiculo", 2), ("comprar-vehiculo", 3)],
    };

    let run = |providers: &[&dyn search::engine::CandidateProvider]| {
        let normalized = search::tokenizer::tokenize("compre un auto usado", &fixture.synonyms);
        let candidates: Vec<search::types::Candidate> = providers
            .iter()
            .flat_map(|provider| {
                support::block_on(provider.candidates(support::STUB_GENERATION, &normalized))
                    .expect("stub providers must not fail")
            })
            .collect();
        engine.score(&normalized, candidates)
    };

    let forward = run(&[&fts, &trigram]);
    let reversed = run(&[&trigram, &fts]);

    assert_eq!(forward, run(&[&fts, &trigram]));
    assert_eq!(
        forward, reversed,
        "provider-list order must never reach the scoring stage"
    );
}
