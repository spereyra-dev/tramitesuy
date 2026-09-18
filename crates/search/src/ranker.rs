//! Ranking (SE-7, SE-8, task 14): merge per-event taxonomy-derived scores
//! with provider-sourced candidates into one ranked list, ordered by score
//! descending with equal scores broken by event slug ascending. Provider
//! contributions are preserved per event under the provider's own rule
//! name (`FTS_TEXT`, `TRIGRAM`), keeping every result hand-reconstructible.

use std::collections::BTreeMap;

use crate::types::{Candidate, EventScore, Explanation, NormalizedQuery, ScoredEvent};

/// Merges taxonomy-derived event scores and provider candidates, then ranks
/// them: score descending, ties broken by slug ascending (SE-8). Events
/// with no entries and no candidates produce no result.
pub fn rank(
    query: &NormalizedQuery,
    event_scores: &[EventScore],
    candidates: &[Candidate],
) -> Vec<ScoredEvent> {
    let mut by_slug: std::collections::BTreeMap<String, Vec<crate::types::ScoreEntry>> =
        BTreeMap::new();
    for score in event_scores {
        by_slug
            .entry(score.slug.clone())
            .or_default()
            .extend(score.entries.iter().cloned());
    }
    for candidate in candidates {
        by_slug
            .entry(candidate.event_slug.clone())
            .or_default()
            .push(crate::types::ScoreEntry {
                rule_name: candidate.rule_name.clone(),
                term: None,
                canonical: None,
                value: candidate.value,
            });
    }

    let mut ranked: Vec<ScoredEvent> = by_slug
        .into_iter()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(slug, entries)| {
            let score = entries.iter().map(|entry| entry.value).sum();
            ScoredEvent {
                slug,
                score,
                explanation: Explanation {
                    tokens: query.tokens.clone(),
                    entries,
                },
            }
        })
        .collect();

    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.slug.cmp(&b.slug)));
    ranked
}
