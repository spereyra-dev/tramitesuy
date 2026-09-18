//! Deterministic run summary types (design §3, spec IN-10): validation
//! findings (skipped rows, duplicate ids) are warnings collected here, not
//! hard errors; structural problems stay hard errors.

/// Timestamp of an ingestion run (RFC 3339 string; wall-clock only at the
/// `apps/ingest` boundary, keeping this crate deterministic).
pub type RunStamp = String;

/// One procedure batch row handed to [`ProcedureRepository`](crate::ports::ProcedureRepository).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureUpsert {
    pub external_id: String,
    pub name: String,
    pub description: String,
    pub organization_external_id: String,
    pub organization_name: String,
    pub official_url: String,
    pub content_hash: String,
    pub raw_data: serde_json::Value,
}

/// Counts returned by a repository upsert batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpsertCounts {
    pub inserted: usize,
    pub updated: usize,
}

/// One validation warning (skip, duplicate resolution, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunWarning {
    /// A row was skipped for missing required fields (IN-4).
    SkippedRow { id: Option<String>, reason: String },
    /// A duplicate `id` was resolved deterministically (IN-5): names the id,
    /// the winner, and the losers.
    DuplicateId {
        id: String,
        winner: String,
        losers: Vec<String>,
    },
}

/// Deterministic, per-run summary (IN-10): counts sum to rows read for the
/// same input, and identical input yields a byte-identical report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummary {
    pub rows_read: usize,
    pub rows_skipped: usize,
    pub duplicates_resolved: usize,
    pub warnings: Vec<RunWarning>,
}
