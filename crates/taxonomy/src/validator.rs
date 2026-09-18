//! Aggregating validator (TX-1, TX-3, TX-4, TX-6, D-2): runs every check
//! over the loaded taxonomy and collects ALL failures, each naming the
//! offending file and value. Nothing short-circuits, so a contributor sees
//! every problem in one pass.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::error::TaxonomyError;
use crate::loader::{load_data_dir, load_external_ids};
use crate::model::Taxonomy;

/// Validates a data directory without the external-id snapshot: the orphan
/// check is skipped entirely (D-2 — orphan validation is snapshot-based; a
/// directory validated without a snapshot cannot know the real id set).
pub fn validate_dir(data_dir: &Path) -> Vec<TaxonomyError> {
    match load_data_dir(data_dir) {
        Ok(taxonomy) => validate(&taxonomy, None),
        Err(error) => vec![error],
    }
}

/// Validates a data directory against a committed external-id snapshot
/// file (D-2): enables the orphan-procedure-reference check.
pub fn validate_dir_against_snapshot(data_dir: &Path, snapshot: &Path) -> Vec<TaxonomyError> {
    match (load_data_dir(data_dir), load_external_ids(snapshot)) {
        (Ok(taxonomy), Ok(external_ids)) => validate(&taxonomy, Some(&external_ids)),
        (Err(error), _) | (_, Err(error)) => vec![error],
    }
}

/// Core aggregation over an already-loaded taxonomy. The orphan check runs
/// only when a snapshot set is supplied (D-2).
pub fn validate(taxonomy: &Taxonomy, external_ids: Option<&HashSet<String>>) -> Vec<TaxonomyError> {
    let mut errors = Vec::new();

    // TX-4: hyphen slug convention for every public slug.
    for source in &taxonomy.events {
        check_slug(&mut errors, &source.file, &source.event.slug);
    }
    for source in &taxonomy.categories {
        check_slug(&mut errors, &source.file, &source.category.slug);
    }

    // TX-3: duplicate event and category slugs, naming every file.
    push_duplicates(
        &mut errors,
        taxonomy
            .events
            .iter()
            .map(|s| (s.event.slug.as_str(), s.file.as_str())),
        |slug, files| TaxonomyError::DuplicateEventSlug {
            slug: slug.to_string(),
            files,
        },
    );
    push_duplicates(
        &mut errors,
        taxonomy
            .categories
            .iter()
            .map(|s| (s.category.slug.as_str(), s.file.as_str())),
        |slug, files| TaxonomyError::DuplicateCategorySlug {
            slug: slug.to_string(),
            files,
        },
    );

    // Category references must resolve to a defined category (TX-3).
    let category_slugs: HashSet<&str> = taxonomy
        .categories
        .iter()
        .map(|s| s.category.slug.as_str())
        .collect();
    for source in &taxonomy.events {
        if !category_slugs.contains(source.event.category.as_str()) {
            errors.push(TaxonomyError::UnknownCategory {
                file: source.file.clone(),
                value: source.event.category.clone(),
            });
        }
    }

    // TX-6 + TX-3 + D-2: relation order uniqueness, and orphan external ids
    // against the snapshot when one is supplied.
    for source in &taxonomy.events {
        let mut seen_orders: HashSet<u32> = HashSet::new();
        for relation in &source.event.relations {
            if !seen_orders.insert(relation.order) {
                errors.push(TaxonomyError::DuplicateRelationOrder {
                    file: source.file.clone(),
                    event: source.event.slug.clone(),
                    order: relation.order,
                });
            }
            if external_ids.is_some_and(|ids| !ids.contains(&relation.external_id)) {
                errors.push(TaxonomyError::OrphanExternalId {
                    file: source.file.clone(),
                    external_id: relation.external_id.clone(),
                });
            }
        }
    }

    errors
}

fn check_slug(errors: &mut Vec<TaxonomyError>, file: &str, slug: &str) {
    if !is_valid_slug(slug) {
        errors.push(TaxonomyError::InvalidSlug {
            file: file.to_string(),
            value: slug.to_string(),
            suggestion: slug.replace('_', "-"),
        });
    }
}

fn push_duplicates<'a, I, F>(errors: &mut Vec<TaxonomyError>, items: I, build: F)
where
    I: Iterator<Item = (&'a str, &'a str)>,
    F: Fn(&str, Vec<String>) -> TaxonomyError,
{
    let mut by_value: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (value, file) in items {
        by_value.entry(value).or_default().push(file.to_string());
    }
    for (value, files) in by_value {
        if files.len() > 1 {
            errors.push(build(value, files));
        }
    }
}

/// The published slug pattern `^[a-z0-9]+(-[a-z0-9]+)*$`: ASCII lowercase
/// letters and digits separated by single hyphens, no leading/trailing
/// hyphen, no consecutive hyphens (TX-4).
pub fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}
