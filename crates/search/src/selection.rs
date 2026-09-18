//! Selection strategy thresholds (SE-10, task 17): a pure function of the
//! confidence, the top1 score, and the D-1 constants. Bands are inclusive
//! exactly as specced: `confidence >= 0.75` with a qualifying top1 score
//! opens the event directly; otherwise `confidence >= 0.40` presents the
//! "¿Te referías a...?" band with up to 3 top-scored events; anything below
//! (or no candidates at all) falls to the related-categories no-result path.

use std::collections::BTreeSet;

use crate::constants::{
    CONFIDENCE_DISAMBIGUATION_THRESHOLD, CONFIDENCE_OPEN_THRESHOLD, MIN_OPEN_SCORE,
};
use crate::types::{ScoredEvent, Selection, SelectionMode};

/// Maximum number of options presented in the disambiguation band (fewer
/// when fewer candidates exist).
pub const MAX_DISAMBIGUATION_OPTIONS: usize = 3;

/// Applies the SE-10 selection strategy to a ranked result list. `results`
/// must already be ranked (score descending); the top-scored prefix feeds
/// both the open decision and the disambiguation options.
pub fn select(confidence: f64, results: &[ScoredEvent], categories: &[String]) -> Selection {
    // Zero positive candidates (confidence 0.0) is always the no-result
    // path, even when negative-scored events are present in the list.
    if results.is_empty() || confidence < CONFIDENCE_DISAMBIGUATION_THRESHOLD {
        let unique: BTreeSet<&String> = categories.iter().collect();
        return Selection {
            mode: SelectionMode::Categories,
            event_slug: None,
            options: Vec::new(),
            categories: unique.into_iter().cloned().collect(),
        };
    }

    let top1 = &results[0];
    if confidence >= CONFIDENCE_OPEN_THRESHOLD && top1.score >= MIN_OPEN_SCORE {
        Selection {
            mode: SelectionMode::Open,
            event_slug: Some(top1.slug.clone()),
            options: Vec::new(),
            categories: Vec::new(),
        }
    } else {
        Selection {
            mode: SelectionMode::Disambiguation,
            event_slug: None,
            options: results
                .iter()
                .take(MAX_DISAMBIGUATION_OPTIONS)
                .cloned()
                .collect(),
            categories: Vec::new(),
        }
    }
}
