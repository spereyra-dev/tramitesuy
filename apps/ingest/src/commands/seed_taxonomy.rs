//! `ingest seed-taxonomy` (task 69): loads the YAML taxonomy through
//! `crates/taxonomy`, validates it against the external-id snapshot (D-2),
//! and projects it into the database through `crates/db::repos::
//! taxonomy_seed` — categories, life_events, keywords, synonyms, relations
//! with `order_index` preserved, idempotent per slug.
//!
//! F14: the daemon shares this exact load → validate → project sequence
//! (`load_validate_seed`), used for its boot bootstrap and its per-cycle
//! re-seed. The CLI keeps its exit codes and its `error: <message>` output;
//! the helper itself never exits or panics. The CLI runs `load_and_validate`
//! before it connects the pool, so an invalid taxonomy fails with its
//! taxonomy error even when the database is unreachable.

use crate::support;
use db::repos::taxonomy_seed::SeedReport;
use std::path::Path;
use taxonomy::model::Taxonomy;

/// Loads the YAML taxonomy and gates it against the external-id snapshot
/// (TX-3). Split out of `load_validate_seed` so the CLI can run the taxonomy
/// checks BEFORE it connects the pool: an invalid taxonomy must print its
/// errors and exit 1 even when the database is unreachable.
pub fn load_and_validate(data_dir: &Path, snapshot: &Path) -> Result<Taxonomy, Vec<String>> {
    let taxonomy =
        taxonomy::loader::load_data_dir(data_dir).map_err(|error| vec![error.to_string()])?;
    // Structural validation gates the projection (duplicates, orphans,
    // slugs — TX-3): a taxonomy that fails `taxonomy-validate` cannot seed.
    let failures = taxonomy::validator::validate_dir_against_snapshot(data_dir, snapshot);
    if !failures.is_empty() {
        return Err(failures
            .into_iter()
            .map(|failure| failure.to_string())
            .collect());
    }
    Ok(taxonomy)
}

/// The shared load → validate → project sequence (F14): parse the YAML,
/// gate it against the external-id snapshot (TX-3), and project it in one
/// transaction. Every failure is returned as its message(s) — never a panic
/// or a process exit — so the CLI (which prints each and exits 1) and the
/// daemon (which records a failed cycle) can each decide.
pub async fn load_validate_seed(
    pool: &sqlx::PgPool,
    data_dir: &Path,
    snapshot: &Path,
) -> Result<SeedReport, Vec<String>> {
    let taxonomy = load_and_validate(data_dir, snapshot)?;
    db::repos::taxonomy_seed::seed_taxonomy(pool, &taxonomy)
        .await
        .map_err(|error| vec![error.to_string()])
}

pub fn run(data_dir: &str, snapshot: &str, database_url: Option<&str>) {
    // The taxonomy checks run first, with no pool yet: an invalid taxonomy
    // with an unreachable database must still print the taxonomy error and
    // exit 1 instead of panicking in `connect_pool`.
    let taxonomy = match load_and_validate(Path::new(data_dir), Path::new(snapshot)) {
        Ok(taxonomy) => taxonomy,
        Err(errors) => {
            for error in errors {
                eprintln!("error: {error}");
            }
            std::process::exit(1);
        }
    };
    let pool = support::connect_pool(&support::database_url(database_url));
    match support::block_on(db::repos::taxonomy_seed::seed_taxonomy(&pool, &taxonomy)) {
        Ok(report) => print!("{}", report.report()),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
