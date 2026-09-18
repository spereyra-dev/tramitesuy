//! Community taxonomy: YAML sources of truth for life events, typed keywords,
//! synonyms, and categories, with strict validation (deny_unknown_fields,
//! hyphen-slug convention, duplicate and orphan detection).
//!
//! This crate is pure: no database, no HTTP; filesystem access is confined
//! to the loader entry points, the validator directory wrappers, and the
//! `taxonomy-validate` CLI bin.

pub mod error;
pub mod loader;
pub mod model;
pub mod validator;
