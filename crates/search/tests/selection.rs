//! Selection contract (SE-10, task 17): a pure function of the confidence,
//! the top1 score, and the D-1 constants. Band edges are inclusive exactly
//! as specced: 0.75 open / 0.40 disambiguation / below 0.40 categories.

use search::constants::CONFIDENCE_OPEN_THRESHOLD;
use search::selection::select;
use search::types::{Explanation, ScoredEvent, SelectionMode};

fn scored(slug: &str, score: i64) -> ScoredEvent {
    ScoredEvent {
        slug: slug.to_string(),
        score,
        explanation: Explanation {
            tokens: Vec::new(),
            entries: Vec::new(),
        },
    }
}

fn fixture_categories() -> Vec<String> {
    vec!["vehiculos".to_string()]
}

#[test]
fn opens_at_the_exact_open_threshold() {
    // GIVEN confidence exactly 0.75 with a qualifying top1 score,
    // WHEN selection runs, THEN the result is open-direct.
    let selection = select(
        CONFIDENCE_OPEN_THRESHOLD,
        &[scored("comprar-vehiculo", 10)],
        &fixture_categories(),
    );
    assert_eq!(selection.mode, SelectionMode::Open);
    assert_eq!(selection.event_slug.as_deref(), Some("comprar-vehiculo"));
}

#[test]
fn disambiguates_at_the_exact_lower_edge() {
    // GIVEN confidence exactly 0.40, WHEN selection runs, THEN the result
    // is disambiguation with up to 3 top-scored events.
    let selection = select(
        0.40,
        &[
            scored("a", 20),
            scored("b", 19),
            scored("c", 18),
            scored("d", 17),
        ],
        &fixture_categories(),
    );
    assert_eq!(selection.mode, SelectionMode::Disambiguation);
    assert_eq!(
        selection
            .options
            .iter()
            .map(|option| option.slug.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"],
        "disambiguation must carry the top 3 scored events"
    );
}

#[test]
fn disambiguation_options_are_fewer_when_fewer_exist() {
    let selection = select(
        0.40,
        &[scored("a", 20), scored("b", 19)],
        &fixture_categories(),
    );
    assert_eq!(selection.mode, SelectionMode::Disambiguation);
    assert_eq!(selection.options.len(), 2);
}

#[test]
fn below_the_lower_edge_falls_to_related_categories() {
    let selection = select(
        0.3999,
        &[scored("a", 20), scored("b", 19)],
        &fixture_categories(),
    );
    assert_eq!(selection.mode, SelectionMode::Categories);
    assert_eq!(selection.event_slug, None);
    assert!(selection.options.is_empty());
    assert_eq!(selection.categories, vec!["vehiculos".to_string()]);
}

#[test]
fn weak_single_candidate_lands_in_disambiguation() {
    // Spec scenario "weak single candidate does not open": despite
    // confidence 0.80, the top1 score (3) is below MIN_OPEN_SCORE (= 10),
    // so the single candidate becomes the band's only option.
    let selection = select(0.80, &[scored("solo", 3)], &fixture_categories());
    assert_eq!(selection.mode, SelectionMode::Disambiguation);
    assert_eq!(selection.options.len(), 1);
    assert_eq!(selection.options[0].slug, "solo");
    assert_eq!(selection.options[0].score, 3);
    assert_eq!(selection.event_slug, None);
}

#[test]
fn a_high_confidence_weak_top1_does_not_open() {
    // The open band requires BOTH the confidence threshold and
    // MIN_OPEN_SCORE; a confident but weak top1 stays in disambiguation.
    let selection = select(0.90, &[scored("weak", 3)], &fixture_categories());
    assert_eq!(selection.mode, SelectionMode::Disambiguation);
}

#[test]
fn zero_candidates_take_the_categories_no_result_path() {
    let selection = select(0.0, &[], &fixture_categories());
    assert_eq!(selection.mode, SelectionMode::Categories);
    assert_eq!(selection.event_slug, None);
    assert!(selection.options.is_empty());
    assert_eq!(selection.categories, vec!["vehiculos".to_string()]);
}

#[test]
fn categories_are_sorted_and_deduplicated() {
    let categories = vec![
        "vivienda".to_string(),
        "vehiculos".to_string(),
        "vivienda".to_string(),
    ];
    let selection = select(0.0, &[], &categories);
    assert_eq!(
        selection.categories,
        vec!["vehiculos".to_string(), "vivienda".to_string()],
        "the categories payload must be deterministic (sorted, unique)"
    );
}
