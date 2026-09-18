//! Community taxonomy: YAML sources of truth for life events, typed keywords,
//! synonyms, and categories, with strict validation (deny_unknown_fields,
//! hyphen-slug convention, duplicate and orphan detection).
//!
//! This crate is pure: no database, no HTTP, no filesystem outside the loader
//! entry points and tests.

pub mod placeholder;
