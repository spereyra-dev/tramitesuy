//! `ingest seed-taxonomy` (task 69): loads the YAML taxonomy through
//! `crates/taxonomy`, validates it against the external-id snapshot (D-2),
//! and projects it into the database through `crates/db::repos::
//! taxonomy_seed` — categories, life_events, keywords, synonyms, relations
//! with `order_index` preserved, idempotent per slug.

use crate::support;

pub fn run(data_dir: &str, snapshot: &str, database_url: Option<&str>) {
    let path = std::path::Path::new(data_dir);
    let taxonomy = match taxonomy::loader::load_data_dir(path) {
        Ok(taxonomy) => taxonomy,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    };
    // Structural validation gates the projection (duplicates, orphans,
    // slugs — TX-3): a taxonomy that fails `taxonomy-validate` cannot seed.
    let failures =
        taxonomy::validator::validate_dir_against_snapshot(path, std::path::Path::new(snapshot));
    if !failures.is_empty() {
        for failure in failures {
            eprintln!("error: {failure}");
        }
        std::process::exit(1);
    }

    let pool = support::connect_pool(&support::database_url(database_url));
    match support::block_on(db::repos::taxonomy_seed::seed_taxonomy(&pool, &taxonomy)) {
        Ok(report) => print!("{}", report.report()),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
