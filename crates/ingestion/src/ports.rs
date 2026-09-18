//! Port traits for the ingestion pipeline (design §3, D-5): the pipeline
//! speaks only through these; real adapters (CKAN HTTP, sqlx repositories)
//! live in `apps/ingest` and `crates/db`, keeping unit tests fixture-driven
//! and network-free.

use crate::error::FetchError;
use crate::row::RawRow;
use crate::summary::ProcedureUpsert;
use std::collections::HashMap;

/// Parses raw source bytes into [`RawRow`]s (RFC 4180-safe for CSV).
///
/// Alternate formats (e.g. XLSX) implement this trait without restructuring
/// the pipeline (spec IN-3).
pub trait FormatStrategy {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<RawRow>, crate::error::ParseError>;
}

/// Resolves and downloads the source dataset. `resolve_dataset` MUST hit
/// CKAN `package_show` at call time — no URL literals may exist in code
/// (spec IN-2).
pub trait SourceFetcher {
    fn resolve_dataset(&self) -> Result<DatasetManifest, FetchError>;
    fn download_resource(&self, resource_id: &str) -> Result<Vec<u8>, FetchError>;
}

/// Manifest describing the resolved source resource; `last_modified` and
/// `hash` feed change detection (spec IN-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetManifest {
    pub resource_id: String,
    pub last_modified: String,
    pub hash: String,
}

/// Storage-agnostic persistence port implemented by `crates/db` in
/// production and by an in-memory fake in tests.
pub trait ProcedureRepository {
    /// external_id → latest open content_hash.
    fn latest_hashes(&self) -> Result<HashMap<String, String>, crate::error::RepoError>;
    /// Inserts/updates procedure rows for one batch (single tx per batch).
    fn upsert_procedures(
        &self,
        rows: &[ProcedureUpsert],
    ) -> Result<crate::summary::UpsertCounts, crate::error::RepoError>;
    /// Closes the currently open version of each (external_id, hash) pair at
    /// the given timestamp.
    fn close_versions(
        &self,
        ids: &[(String, String)],
        at: crate::summary::RunStamp,
    ) -> Result<(), crate::error::RepoError>;
    /// Marks procedures absent from the source as inactive (never deletes).
    fn deactivate_missing(
        &self,
        present_ids: &std::collections::BTreeSet<String>,
        at: crate::summary::RunStamp,
    ) -> Result<usize, crate::error::RepoError>;
    /// Advances `last_seen_at` for the given ids.
    fn touch_last_seen(
        &self,
        ids: &[String],
        at: crate::summary::RunStamp,
    ) -> Result<(), crate::error::RepoError>;
    /// Every known external_id (feeds `export-ids`).
    fn all_external_ids(&self) -> Result<Vec<String>, crate::error::RepoError>;
}
