//! ACTION_ENTITY combination bonuses (SE-5, task 12): a rule's bonus is
//! added once, under the rule name `ACTION_ENTITY`, only when the same
//! query matches both the action and the entity term. One-sided matches
//! earn nothing.

use crate::matcher::matches;
use crate::types::{CombinationRule, NormalizedQuery, ScoreEntry};

/// Rule name for the ACTION_ENTITY combination bonus (SE-5).
pub const ACTION_ENTITY_RULE_NAME: &str = "ACTION_ENTITY";

/// Evaluates every rule against the query tokens, emitting one entry per
/// satisfied rule in rule declaration order. Each rule fires at most once,
/// however many tokens match each side.
pub fn action_entity_entries(
    query: &NormalizedQuery,
    rules: &[CombinationRule],
) -> Vec<ScoreEntry> {
    rules
        .iter()
        .filter_map(|rule| {
            let action_matched = query
                .tokens
                .iter()
                .any(|token| matches(&token.canonical, &rule.action));
            let entity_matched = query
                .tokens
                .iter()
                .any(|token| matches(&token.canonical, &rule.entity));
            (action_matched && entity_matched).then(|| ScoreEntry {
                rule_name: ACTION_ENTITY_RULE_NAME.to_string(),
                term: Some(rule.action.clone()),
                canonical: Some(rule.entity.clone()),
                value: rule.bonus,
            })
        })
        .collect()
}
