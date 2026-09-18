//! Typed errors for the ingestion crate (design §3 error strategy:
//! validation problems are warnings, structural problems are hard errors).

use thiserror::Error;

/// Errors raised while parsing a source payload through a
/// [`FormatStrategy`](crate::ports::FormatStrategy).
#[derive(Debug, Error)]
pub enum ParseError {
    /// The payload is not valid UTF-8, or is structurally broken CSV.
    #[error("source payload is not parseable: {0}")]
    Malformed(String),
}

/// Errors raised while fetching source bytes through
/// [`SourceFetcher`](crate::ports::SourceFetcher).
#[derive(Debug, Error)]
pub enum FetchError {
    /// The fetcher failed before or during download.
    #[error("source fetch failed: {0}")]
    Failed(String),
}

/// Errors raised by a [`ProcedureRepository`](crate::ports::ProcedureRepository).
#[derive(Debug, Error)]
pub enum RepoError {
    /// The repository failed to apply a persistence step.
    #[error("repository operation failed: {0}")]
    Failed(String),
}

/// Top-level ingestion failure: only structural problems reach here.
#[derive(Debug, Error)]
pub enum IngestionError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error(transparent)]
    Repo(#[from] RepoError),
}
