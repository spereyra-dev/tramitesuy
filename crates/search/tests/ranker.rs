//! Ranking contract (SE-7, SE-8, task 14): candidates from every provider
//! merge into one ranked list ordered by score descending, equal scores
//! break by event slug ascending, and provider-sourced entries are preserved
//! per event under their own rule names.

mod support;

use search::ranker::rank;
use search::tokenizer::tokenize;
use search::types::{Candidate, EventScore, ScoreEntry};

fn keyword_entry(term: &str, value: i64) -> ScoreEntry {
    ScoreEntry {
        rule_name: "KEYWORD".to_string(),
        term: Some(term.to_string()),
        canonical: Some(term.to_string()),
        value,
    }
}

fn candidate(slug: &str, rule_name: &str, value: i64) -> Candidate {
    Candidate {
        event_slug: slug.to_string(),
        rule_name: rule_name.to_string(),
        value,
    }
}

#[test]
fn provider_candidates_merge_into_one_ranked_list() {
    let fixture = support::vehiculos_fixture();
    let query = tokenize("compre un auto usado", &fixture.synonyms);
    let event_scores = vec![EventScore {
        slug: "comprar-vehiculo".to_string(),
        entries: vec![keyword_entry("comprar", 10)],
    }];
    let candidates = vec![
        candidate("comprar-vehiculo", "FTS_TEXT", 5),
        candidate("comprar-vehiculo", "TRIGRAM", 2),
        candidate("vender-vehiculo", "FTS_TEXT", 5),
    ];

    let ranked = rank(&query, &event_scores, &candidates);

    assert_eq!(ranked.len(), 2, "one merged result per scored event");
    assert_eq!(ranked[0].slug, "comprar-vehiculo");
    assert_eq!(ranked[0].score, 17, "keyword 10 + FTS_TEXT 5 + TRIGRAM 2");
    let rule_names: Vec<&str> = ranked[0]
        .explanation
        .entries
        .iter()
        .map(|entry| entry.rule_name.as_str())
        .collect();
    assert!(
        rule_names.contains(&"FTS_TEXT") && rule_names.contains(&"TRIGRAM"),
        "provider-sourced entries must be preserved per event"
    );
    assert_eq!(ranked[1].slug, "vender-vehiculo");
    assert_eq!(ranked[1].score, 5);
    for scored in &ranked {
        let sum: i64 = scored.explanation.entries.iter().map(|e| e.value).sum();
        assert_eq!(sum, scored.score, "explanation must reconstruct the score");
    }
}

#[test]
fn equal_scores_order_by_event_slug_ascending() {
    let query = search::normalizer::normalize("auto");
    let event_scores = vec![
        EventScore {
            slug: "vender-vehiculo".to_string(),
            entries: vec![keyword_entry("vehiculo", 8)],
        },
        EventScore {
            slug: "comprar-vehiculo".to_string(),
            entries: vec![keyword_entry("vehiculo", 8)],
        },
    ];

    let ranked = rank(&query, &event_scores, &[]);

    let slugs: Vec<&str> = ranked.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(slugs, vec!["comprar-vehiculo", "vender-vehiculo"]);
}

#[test]
fn higher_scores_rank_first() {
    let query = search::normalizer::normalize("auto");
    let event_scores = vec![
        EventScore {
            slug: "comprar-vehiculo".to_string(),
            entries: vec![keyword_entry("vehiculo", 8)],
        },
        EventScore {
            slug: "vender-vehiculo".to_string(),
            entries: vec![keyword_entry("vehiculo", 20)],
        },
    ];

    let ranked = rank(&query, &event_scores, &[]);

    let slugs: Vec<&str> = ranked.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(slugs, vec!["vender-vehiculo", "comprar-vehiculo"]);
}

#[test]
fn empty_inputs_yield_no_results() {
    let query = search::normalizer::normalize("auto");

    assert!(rank(&query, &[], &[]).is_empty(), "no entries, no results");
}
