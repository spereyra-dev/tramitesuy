//! Pure deterministic search engine for TrámitesUY.
//!
//! This crate is the product core: it maps a citizen's life-situation query
//! ("compré un auto usado") onto a life event via transparent, explainable,
//! lexicon-based scoring. It must stay pure:
//!
//! - no database access,
//! - no HTTP clients,
//! - no async runtimes,
//! - no filesystem access outside tests,
//! - no generative AI or embeddings (the `CandidateProvider` seam stays empty).
//!
//! Purity is enforced by `tests/no_forbidden_deps.rs`.

pub mod confidence;
pub mod constants;
pub mod engine;
pub mod golden;
pub mod matcher;
pub mod normalizer;
pub mod ranker;
pub mod rules;
pub mod selection;
pub mod tokenizer;
pub mod types;
