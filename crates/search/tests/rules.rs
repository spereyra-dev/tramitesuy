//! ACTION_ENTITY combination bonus (SE-5, task 12): the bonus applies only
//! when both the action and the entity are matched by the same query, and
//! at most once per rule. Also covers the negative-keyword penalty case
//! (SE-6, task 13) through the full per-event scoring composition.

mod support;

use search::matcher::{KEYWORD_RULE_NAME, NEGATIVE_KEYWORD_RULE_NAME, match_keywords};
use search::rules::{ACTION_ENTITY_RULE_NAME, action_entity_entries};
use search::tokenizer::tokenize;
use search::types::ScoreEntry;

fn bonus_entry(action: &str, entity: &str, bonus: i64) -> ScoreEntry {
    ScoreEntry {
        rule_name: ACTION_ENTITY_RULE_NAME.to_string(),
        term: Some(action.to_string()),
        canonical: Some(entity.to_string()),
        value: bonus,
    }
}

#[test]
fn bonus_applies_to_the_full_combination() {
    let fixture = support::vehiculos_fixture();
    let lex = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("compre un auto", &fixture.synonyms);

    let entries = action_entity_entries(&query, &lex.rules);

    assert_eq!(entries, vec![bonus_entry("comprar", "vehiculo", 15)]);
}

#[test]
fn entity_only_query_gets_no_bonus() {
    let fixture = support::vehiculos_fixture();
    let lex = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("auto usado", &fixture.synonyms);

    let entries = action_entity_entries(&query, &lex.rules);

    assert!(
        entries.is_empty(),
        "entity-only query must not earn the bonus"
    );
}

#[test]
fn opposite_action_is_penalized() {
    // Task 13: `comprar-vehiculo` declares `vender: -15`; `vendi mi auto`
    // must produce `NEGATIVE_KEYWORD vender -15` and no KEYWORD entry for
    // the negative term.
    let fixture = support::vehiculos_fixture();
    let lex = support::event_lexicon(&fixture.events[0]);
    let query = tokenize("vendi mi auto", &fixture.synonyms);

    let mut entries = match_keywords(&query, &lex.keywords);
    entries.extend(action_entity_entries(&query, &lex.rules));

    let penalty = entries
        .iter()
        .find(|entry| {
            entry.rule_name == NEGATIVE_KEYWORD_RULE_NAME && entry.term.as_deref() == Some("vender")
        })
        .expect("vender penalty must be reported for comprar-vehiculo");
    assert_eq!(penalty.value, -15);
    assert!(
        !entries
            .iter()
            .any(|entry| entry.rule_name == KEYWORD_RULE_NAME
                && entry.term.as_deref() == Some("vender")),
        "a negative keyword must never also score as a positive KEYWORD"
    );
}

#[test]
fn bonus_fires_at_most_once_per_rule() {
    let fixture = support::vehiculos_fixture();
    let lex = support::event_lexicon(&fixture.events[0]);
    // Three synonym surfaces all canonicalize to `vehiculo`; the rule still
    // fires exactly once.
    let query = tokenize("compre un auto coche vehiculo", &fixture.synonyms);

    let entries = action_entity_entries(&query, &lex.rules);

    assert_eq!(entries, vec![bonus_entry("comprar", "vehiculo", 15)]);
}

#[test]
fn another_events_rule_does_not_fire() {
    // `vender-vehiculo` declares `vender + vehiculo -> +15`; an unrelated
    // buying query matches neither its action nor its rule.
    let fixture = support::vehiculos_fixture();
    let lex = support::event_lexicon(&fixture.events[1]);
    let query = tokenize("compre un auto", &fixture.synonyms);

    let entries = action_entity_entries(&query, &lex.rules);

    assert!(entries.is_empty(), "the bonus is per-event, not global");
}
