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

/// Deterministic, per-run summary (IN-10): the row-derived counts sum to
/// `rows_read` for the same input, and identical input yields a
/// byte-identical [`report()`](RunSummary::report).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummary {
    pub rows_read: usize,
    pub rows_skipped: usize,
    /// Source rows eliminated by duplicate resolution (IN-5 losers): the
    /// row-accounting sum needs rows, not the count of resolved ids.
    pub duplicates_resolved: usize,
    /// Procedures persisted for the first time this run.
    pub created: usize,
    /// Procedures re-persisted with a changed content hash.
    pub updated: usize,
    /// Procedures whose content hash matched — no version row (IN-9).
    pub unchanged: usize,
    /// Previously known procedures absent from this run's source —
    /// deactivated, never deleted (IN-7). This counts procedures, not
    /// source rows, so it sits outside the row-accounting sum.
    pub deactivated: usize,
    pub warnings: Vec<RunWarning>,
}

impl RunSummary {
    /// Records skipped rows as warnings (design §3 error strategy: skip
    /// findings are warnings, never hard errors).
    pub fn record_skips(&mut self, skipped: &[crate::row::SkippedRow]) {
        self.rows_skipped += skipped.len();
        self.warnings
            .extend(skipped.iter().map(|s| RunWarning::SkippedRow {
                id: s.id.clone(),
                reason: s.reason.clone(),
            }));
    }

    /// Records duplicate-resolution warnings (IN-5) plus the number of
    /// source rows those duplicates eliminated, which feeds the IN-10
    /// row-accounting sum.
    pub fn record_duplicates(&mut self, warnings: Vec<RunWarning>, resolved_rows: usize) {
        self.duplicates_resolved += resolved_rows;
        self.warnings.extend(warnings);
    }

    /// Sorts the warnings into a canonical order (skips before duplicates,
    /// each group by id/reason) so the summary is byte-identical regardless
    /// of the source file's row order (SE-1).
    pub fn canonicalize_warnings(&mut self) {
        self.warnings.sort_by_key(|warning| match warning {
            RunWarning::SkippedRow { id, reason } => (
                0u8,
                id.clone().unwrap_or_default(),
                reason.clone(),
                String::new(),
            ),
            RunWarning::DuplicateId { id, .. } => (1u8, id.clone(), String::new(), String::new()),
        });
    }

    /// The row-accounting invariant (IN-10): every row read is either
    /// skipped, resolved as a duplicate loser, or persisted as one of
    /// created / updated / unchanged. `deactivated` counts procedures — not
    /// rows — so it is excluded by definition.
    pub fn accounted_rows(&self) -> usize {
        self.rows_skipped + self.duplicates_resolved + self.created + self.updated + self.unchanged
    }

    /// A deterministic, byte-stable rendering of the summary: one count
    /// line followed by one line per warning, in canonical order.
    pub fn report(&self) -> String {
        let mut out = format!(
            "rows_read={} rows_skipped={} duplicates_resolved={} created={} updated={} unchanged={} deactivated={}\n",
            self.rows_read,
            self.rows_skipped,
            self.duplicates_resolved,
            self.created,
            self.updated,
            self.unchanged,
            self.deactivated,
        );
        for warning in &self.warnings {
            match warning {
                RunWarning::SkippedRow { id, reason } => {
                    out.push_str(&format!("skip id={:?} reason={reason}\n", id));
                }
                RunWarning::DuplicateId { id, winner, losers } => {
                    out.push_str(&format!(
                        "duplicate id={id} winner={winner} losers={losers:?}\n",
                        losers = losers
                    ));
                }
            }
        }
        out
    }
}
