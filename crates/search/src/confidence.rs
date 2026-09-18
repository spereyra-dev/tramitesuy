//! Deterministic confidence formula (SE-9, D-1, task 16).
//!
//! Confidence is never a probabilistic estimate: it is a pure function of
//! the competing positive candidate scores, rounded to two decimals.
//! Negative and zero scores never participate — a single positive candidate
//! takes the `CONFIDENCE_SINGLE_CANDIDATE_FLOOR`, and none takes 0.0.

use crate::constants::CONFIDENCE_SINGLE_CANDIDATE_FLOOR;

/// Computes confidence from a ranked score list (SE-9): with two or more
/// positive candidates `top1 / (top1 + top2)`; with exactly one positive
/// candidate the single-candidate floor; with none, 0.0 (the no-result
/// path). The list's order does not matter — the two largest positive
/// scores are selected internally.
pub fn confidence(scores: &[i64]) -> f64 {
    let mut positive: Vec<i64> = scores.iter().copied().filter(|score| *score > 0).collect();
    match positive.len() {
        0 => 0.0,
        1 => CONFIDENCE_SINGLE_CANDIDATE_FLOOR,
        _ => {
            positive.sort_unstable_by(|a, b| b.cmp(a));
            let (top1, top2) = (positive[0], positive[1]);
            round_two(top1 as f64 / (top1 + top2) as f64)
        }
    }
}

/// Rounds to two decimals via exact f64 formatting (design D-1:
/// round-half-even as produced by `{:.2}` formatting).
pub fn round_two(value: f64) -> f64 {
    format!("{value:.2}").parse::<f64>().unwrap_or(value)
}
