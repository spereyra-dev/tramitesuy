//! sqlx-backed persistence for TrámitesUY: pool setup, repositories for
//! procedures/organizations/taxonomy/search logs/feedback, and the FTS + trigram
//! `CandidateProvider` implementations consumed by the pure `search` engine.
//!
//! Migration files live in `../../migrations` and are embedded with
//! `sqlx::migrate!` (see [`pool::run_migrations`]).

pub mod generations;
pub mod pool;
pub mod providers;
pub mod repos;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use pool::{connect, run_migrations};
