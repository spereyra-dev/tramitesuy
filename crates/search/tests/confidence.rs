//! Confidence contract (SE-9, D-1, task 16): deterministic, rounded to two
//! decimals, derived only from competing positive candidate scores. Negative
//! and zero scores never participate in the ratio.

use search::confidence::confidence;
use search::constants::CONFIDENCE_SINGLE_CANDIDATE_FLOOR;

#[test]
fn dominant_winner_computes_the_top1_ratio() {
    // Spec scenario "dominant winner opens directly": 36 / (36 + 9).
    assert_eq!(confidence(&[36, 9]), 0.80, "36/9 must yield exactly 0.80");
}

#[test]
fn near_tie_falls_in_the_disambiguation_band() {
    // Spec scenario "near-tie is ambiguous": 22 / (22 + 20).
    assert_eq!(confidence(&[22, 20]), 0.52, "22/20 must yield exactly 0.52");
}

#[test]
fn single_positive_candidate_uses_the_floor() {
    // Spec scenario "single candidate uses the floor": 14 >= MIN_OPEN_SCORE.
    assert_eq!(confidence(&[14]), CONFIDENCE_SINGLE_CANDIDATE_FLOOR);
}

#[test]
fn weak_single_candidate_still_reports_the_floor() {
    // Spec scenario "weak single candidate does not open": confidence alone
    // is 0.80, but the MIN_OPEN_SCORE gate belongs to selection (task 17).
    // A score of 3 sits below MIN_OPEN_SCORE (= 10).
    assert_eq!(confidence(&[3]), CONFIDENCE_SINGLE_CANDIDATE_FLOOR);
}

#[test]
fn zero_scoring_candidates_report_zero_confidence() {
    assert_eq!(confidence(&[]), 0.0, "no candidates is the no-result path");
    assert_eq!(
        confidence(&[0]),
        0.0,
        "a zero score is not a positive candidate"
    );
    assert_eq!(
        confidence(&[-7]),
        0.0,
        "a only-negative ranked list is the no-result path"
    );
}

#[test]
fn negative_scores_never_contaminate_the_ratio() {
    // `vendi mi auto` leaves comprar-vehiculo at 8 - 15 = -7 next to
    // vender-vehiculo's 33: only the positive score competes.
    assert_eq!(confidence(&[33, -7]), CONFIDENCE_SINGLE_CANDIDATE_FLOOR);
}

#[test]
fn values_are_rounded_to_two_decimals() {
    assert_eq!(
        confidence(&[22, 20]),
        0.52,
        "0.523809... must round to 0.52"
    );
    assert_eq!(
        confidence(&[100, 33]),
        0.75,
        "0.751879... must round to 0.75"
    );
    assert_eq!(
        confidence(&[1, 2]),
        0.67,
        "top1=2 over 2+1 = 0.6666... rounds to 0.67"
    );
    assert_eq!(confidence(&[1, 3]), 0.75, "top1=3 over 3+1 = 0.75");
}

#[test]
fn input_order_does_not_change_confidence() {
    // The formula reads top1/top2 as the two largest positive scores, so a
    // differently ordered score list must produce the same value.
    assert_eq!(confidence(&[9, 36]), confidence(&[36, 9]));
    assert_eq!(confidence(&[20, 22]), confidence(&[22, 20]));
}
