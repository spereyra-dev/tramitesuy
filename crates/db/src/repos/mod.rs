//! Repositories implementing the ingestion ports over sqlx (design §2, D-5).
//! `crates/db` is the only sqlx-aware crate; the ingestion pipeline keeps
//! speaking to storage through the `ProcedureRepository` port only.

pub mod orgs;
pub mod procedures;
