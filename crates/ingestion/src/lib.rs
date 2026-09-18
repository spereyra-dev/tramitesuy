//! Ingestion pipeline ports and CSV strategy for the AGESIC catalog.
//!
//! Port-driven by design: `SourceFetcher`, `FormatStrategy`, and
//! `ProcedureRepository` are defined here; real adapters (CKAN HTTP, sqlx
//! repositories) live in `apps/ingest` and `crates/db` so unit tests run on
//! fixtures only.

pub mod dedup;
pub mod error;
pub mod format;
pub mod in_memory;
pub mod pipeline;
pub mod ports;
pub mod row;
pub mod summary;

pub use in_memory::{
    InMemoryProcedureRepository, OrganizationRecord, ProcedureRecord, ProcedureStatus,
    VersionRecord,
};
