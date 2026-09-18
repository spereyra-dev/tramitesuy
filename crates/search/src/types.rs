//! Public value types shared by the engine facade and its consumers
//! (task 7). Every score is a sum of named, inspectable contributions and
//! every result is traceable to taxonomy keywords, synonyms, and rules.

/// A single query token: its normalized surface form (`original`) and the
/// synonym-canonicalized form (`canonical`) used for keyword matching (SE-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub original: String,
    pub canonical: String,
}

/// Output of the normalization pipeline (SE-2): the original text verbatim,
/// the normalized text, and the ordered token list after stop-word removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedQuery {
    pub original: String,
    pub normalized: String,
    pub tokens: Vec<Token>,
}

/// One provider-sourced contribution to an event's score (SE-7), reported
/// under the provider's own rule name (`FTS_TEXT`, `TRIGRAM`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub event_slug: String,
    pub rule_name: String,
    pub value: i64,
}

/// One named, inspectable scoring contribution (SE-11). `term`/`canonical`
/// identify the keyword when the entry comes from keyword matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreEntry {
    pub rule_name: String,
    pub term: Option<String>,
    pub canonical: Option<String>,
    pub value: i64,
}

/// The query tokens plus the per-event score entries. The sum of all entry
/// values MUST equal the event's score exactly (SE-11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub tokens: Vec<Token>,
    pub entries: Vec<ScoreEntry>,
}

/// A ranked life event with its score and its reconstructible explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoredEvent {
    pub slug: String,
    pub score: i64,
    pub explanation: Explanation,
}

/// Selection band chosen from confidence and top1 score (SE-10): open the
/// event directly, present "¿Te referías a...?" options, or fall back to
/// related categories (the no-result path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Open,
    Disambiguation,
    Categories,
}

/// Pure selection result carrying the chosen band and its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub mode: SelectionMode,
    /// Set only when `mode` is `Open`.
    pub event_slug: Option<String>,
    /// Up to 3 top-scored events when `mode` is `Disambiguation`.
    pub options: Vec<ScoredEvent>,
    /// Available category slugs when `mode` is `Categories`.
    pub categories: Vec<String>,
}

/// Full deterministic search outcome consumed by the API surface.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutcome {
    pub query: NormalizedQuery,
    pub results: Vec<ScoredEvent>,
    pub confidence: f64,
    pub selection: Selection,
}
