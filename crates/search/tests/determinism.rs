//! Determinism contract (SE-1, task 8): identical inputs produce identical
//! output on every run in the same process.
//!
//! Unit A1 covers the deterministic foundations available at this point:
//! normalization and synonym canonicalization over the shared taxonomy
//! fixture. The full-pipeline determinism assertion (scores, ordering,
//! confidence, explanations) extends this test in units A2/A3 (tasks 14, 18)
//! as matcher, ranker, confidence, and selection land.

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
