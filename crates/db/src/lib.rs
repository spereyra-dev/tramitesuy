//! sqlx-backed persistence for TrámitesUY: pool setup, repositories for
//! procedures/organizations/taxonomy/search logs/feedback, and the FTS + trigram
//! `CandidateProvider` implementations consumed by the pure `search` engine.
//!
//! Migration files live in `../../migrations` and are embedded with
//! `sqlx::migrate!` when slice (b) lands.

pub mod placeholder;
