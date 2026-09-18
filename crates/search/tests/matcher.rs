//! Weighted keyword matching (SE-4, task 11): matched keyword weights
//! accumulate under the rule name `KEYWORD`, with synonym canonicalization
//! attaching the weight to the canonical term. Negative-keyword penalties
//! (SE-6) are accumulated by this same pass (design §2, task 13).

mod support;

use search::matcher::{KEYWORD_RULE_NAME, NEGATIVE_KEYWORD_RULE_NAME, match_keywords, matches};
use search::tokenizer::tokenize;
use search::types::{EventLexicon, ScoreEntry};

fn keyword_entry(term: &str, value: i64) -> ScoreEntry {
    ScoreEntry {
        rule_name: KEYWORD_RULE_NAME.to_string(),
        term: Some(term.to_string()),
        canonical: Some(term.to_string()),
        value,
    }
}

#[test]
fn keyword_weights_accumulate_under_the_keyword_rule() {
    let fixture = support::vehiculos_fixture();
    let lex: EventLexicon = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("compre un auto usado", &fixture.synonyms);

    let entries = match_keywords(&query, &lex.keywords);

    assert_eq!(
        entries,
        vec![
            keyword_entry("comprar", 10),
            keyword_entry("vehiculo", 8),
            keyword_entry("usado", 3),
        ],
        "`compre` matches `comprar` (stem rule); `auto` scores as canonical `vehiculo`"
    );
}

#[test]
fn synonym_matches_attach_to_the_canonical_term() {
    // `coche` is not a keyword; its canonical form `vehiculo` is (SE-3).
    let fixture = support::vehiculos_fixture();
    let lex: EventLexicon = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("compre un coche", &fixture.synonyms);

    let entries = match_keywords(&query, &lex.keywords);

    assert_eq!(
        entries,
        vec![keyword_entry("comprar", 10), keyword_entry("vehiculo", 8)]
    );
}

#[test]
fn conjugated_and_plural_forms_match_their_base_term() {
    // Stem rule (documented in matcher.rs): conjugations and plurals of the
    // base term match; distinct verbs and short stems do not.
    for (token, term) in [
        ("compre", "comprar"),
        ("compra", "comprar"),
        ("compro", "comprar"),
        ("comprando", "comprar"),
        ("vendi", "vender"),
        ("vehiculos", "vehiculo"),
        ("usada", "usado"),
    ] {
        assert!(matches(token, term), "{token} must match {term}");
    }
    for (token, term) in [
        ("comer", "comprar"),
        ("usar", "usado"),
        ("iba", "ir"),
        ("venta", "vender"),
    ] {
        assert!(!matches(token, term), "{token} must not match {term}");
    }
}

#[test]
fn each_keyword_contributes_at_most_once() {
    // Every synonym surface of `vehiculo` is present, yet the keyword
    // contributes its weight exactly once.
    let fixture = support::vehiculos_fixture();
    let lex: EventLexicon = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("auto vehiculo coche", &fixture.synonyms);

    let entries = match_keywords(&query, &lex.keywords);

    let vehiculo_hits = entries
        .iter()
        .filter(|entry| entry.term.as_deref() == Some("vehiculo"))
        .count();
    assert_eq!(vehiculo_hits, 1);
}

#[test]
fn negative_keywords_are_never_scored_as_positive() {
    let fixture = support::vehiculos_fixture();
    let lex: EventLexicon = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("vendi mi auto", &fixture.synonyms);

    let entries = match_keywords(&query, &lex.keywords);

    assert_eq!(
        entries,
        vec![
            keyword_entry("vehiculo", 8),
            ScoreEntry {
                rule_name: NEGATIVE_KEYWORD_RULE_NAME.to_string(),
                term: Some("vender".to_string()),
                canonical: Some("vender".to_string()),
                value: -15,
            },
        ],
        "the penalty is reported; `vender` never gains KEYWORD +15"
    );
}
