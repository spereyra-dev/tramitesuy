//! Explanation reconstructs the score (SE-11, task 15): over a table of
//! seed-shaped queries, the sum of explanation entry values equals the
//! reported score exactly.

mod support;

use search::ranker::rank;
use search::tokenizer::tokenize;
use search::types::{EventScore, ScoredEvent};

fn score_all(query_text: &str) -> Vec<ScoredEvent> {
    let fixture = support::vehiculos_fixture();
    let query = tokenize(query_text, &fixture.synonyms);
    let scores: Vec<EventScore> = fixture
        .events
        .iter()
        .map(|event| support::score_event(&query, event))
        .collect();
    rank(&query, &scores, &[])
}

#[test]
fn hand_reconstructible_case_sums_to_36() {
    // `compre un auto usado`: comprar 10 + vehiculo 8 + usado 3 + bonus 15.
    let ranked = score_all("compre un auto usado");
    let comprar = ranked
        .iter()
        .find(|scored| scored.slug == "comprar-vehiculo")
        .expect("comprar-vehiculo must be scored for this query");
    assert_eq!(comprar.score, 36);
    let sum: i64 = comprar.explanation.entries.iter().map(|e| e.value).sum();
    assert_eq!(sum, 36);
}

#[test]
fn explanation_sum_equals_score_for_seed_queries() {
    for query in [
        "compre un auto usado",
        "compre un coche",
        "compro un vehiculo usado",
        "auto usado",
        "vendi mi auto",
        "venta de vehiculos",
        "quiero vender mi coche",
        "tramites de migracion",
    ] {
        for scored in score_all(query) {
            let sum: i64 = scored.explanation.entries.iter().map(|e| e.value).sum();
            assert_eq!(
                sum, scored.score,
                "query {query:?}: explanation for {} must reconstruct its score",
                scored.slug
            );
        }
    }
}

#[test]
fn ranking_is_score_desc_then_slug_ascending() {
    for query in [
        "compre un auto usado",
        "vendi mi auto",
        "auto usado",
        "venta de vehiculos",
    ] {
        let ranked = score_all(query);
        for pair in ranked.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                a.score > b.score || (a.score == b.score && a.slug < b.slug),
                "query {query:?}: {} ({}) must rank before {} ({})",
                a.slug,
                a.score,
                b.slug,
                b.score
            );
        }
    }
}
