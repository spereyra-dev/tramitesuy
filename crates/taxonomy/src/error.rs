//! Typed taxonomy errors (design §3 error strategy): structural problems
//! (unparseable file, unknown YAML field, orphan ref, bad slug, duplicates)
//! are hard errors, and every validation failure names the offending file
//! and value (TX-2, TX-3).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaxonomyError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid taxonomy file {file}: {message}")]
    Parse { file: String, message: String },

    #[error(
        "invalid slug '{value}' in {file}: slugs must match ^[a-z0-9]+(-[a-z0-9]+)*$ — use hyphens instead of underscores, e.g. '{suggestion}'"
    )]
    InvalidSlug {
        file: String,
        value: String,
        suggestion: String,
    },

    #[error("duplicate event slug '{slug}' declared in both {files:?}")]
    DuplicateEventSlug { slug: String, files: Vec<String> },

    #[error("duplicate category slug '{slug}' declared in {files:?}")]
    DuplicateCategorySlug { slug: String, files: Vec<String> },

    #[error(
        "duplicate relation order {order} in {file} (event '{event}'): order must be unique within the event"
    )]
    DuplicateRelationOrder {
        file: String,
        event: String,
        order: u32,
    },

    #[error("unknown category '{value}' referenced in {file}: no category file defines that slug")]
    UnknownCategory { file: String, value: String },

    #[error(
        "orphan external_id '{external_id}' referenced in {file}: not present in the external-id snapshot"
    )]
    OrphanExternalId { file: String, external_id: String },
}
