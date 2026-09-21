//! Durable catalog generations (stage 3, design §1–§2): the worker-side
//! build that computes the manifest identity (OPT-02/OPT-03) and writes the
//! immutable per-generation projections, plus the publication validation
//! gate. The API-side snapshot loading is stage 3's later slice (S7).

pub mod adopt;
pub mod build;
pub mod validate;

/// Engine version pinned in the manifest (design §1.2: "versión de
/// `crates/search` + revisión de algoritmo"). The pure engine crate carries
/// no version constant of its own (and stage 3 must not touch it), so the
/// revision is pinned here and MUST be bumped whenever the engine's ranking
/// semantics change — it is part of the cache key (design §3.2).
pub const ENGINE_VERSION: &str = "search-rules-v1";
