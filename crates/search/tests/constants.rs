//! D-1 constant values must be public, testable values (SE-9, task 7).

use search::constants::{
    CONFIDENCE_DISAMBIGUATION_THRESHOLD, CONFIDENCE_OPEN_THRESHOLD,
    CONFIDENCE_SINGLE_CANDIDATE_FLOOR, MIN_OPEN_SCORE,
};

#[test]
fn confidence_constants_match_design_d1() {
    assert_eq!(CONFIDENCE_OPEN_THRESHOLD, 0.75);
    assert_eq!(CONFIDENCE_DISAMBIGUATION_THRESHOLD, 0.40);
    assert_eq!(CONFIDENCE_SINGLE_CANDIDATE_FLOOR, 0.80);
}

#[test]
fn min_open_score_is_the_smallest_action_weight() {
    assert_eq!(MIN_OPEN_SCORE, 10);
}
