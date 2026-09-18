//! Named, testable scoring and confidence constants (design D-1, SE-9).
//! No other scoring constants exist: tasks encode exactly these four.

/// Confidence at or above this opens the event directly (band is inclusive).
pub const CONFIDENCE_OPEN_THRESHOLD: f64 = 0.75;

/// Lower bound of the "¿Te referías a...?" disambiguation band (inclusive).
pub const CONFIDENCE_DISAMBIGUATION_THRESHOLD: f64 = 0.40;

/// Confidence assigned when exactly one candidate scored positive.
pub const CONFIDENCE_SINGLE_CANDIDATE_FLOOR: f64 = 0.80;

/// Minimum absolute top1 score required to open an event (equals the
/// smallest meaningful ACTION keyword weight in the seed schema).
pub const MIN_OPEN_SCORE: i64 = 10;
