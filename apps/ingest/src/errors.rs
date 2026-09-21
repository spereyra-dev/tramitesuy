//! Typed errors for the `ingest` crate (composition layer). The commands
//! surface composes typed ports and adapters; these variants name the
//! failure classes callers can match on instead of parsing message text.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PublishError {
    /// The YAML taxonomy under the data directory could not be loaded or
    /// hashed (the taxonomy is the build's source of truth, TX-1).
    #[error("taxonomy load/hash failed: {0}")]
    Taxonomy(String),
    /// The ingestion exclusion advisory lock could not be probed or
    /// released on its session.
    #[error("ingestion exclusion: {0}")]
    Exclusion(String),
    /// The `ingestion_runs` record could not be inserted or finished.
    #[error("ingestion run record: {0}")]
    RunRecord(String),
    /// The generation build (projections + manifest) failed.
    #[error("generation build: {0}")]
    Build(String),
    /// The publication validation gate ran but failed structurally.
    #[error("validation execution: {0}")]
    Validation(String),
    /// The reference promotion did not produce a published generation.
    #[error("promotion: {0}")]
    Promotion(String),
}
