//! DB-free validation CLI (TX-3, D-2, task 26):
//! `taxonomy-validate <data-dir> <snapshot-file>` — loads the taxonomy,
//! validates it against the external-id snapshot, and exits non-zero
//! printing every failure (file + value) when validation fails. CI consumes
//! this binary (task 90).

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use taxonomy::error::TaxonomyError;
use taxonomy::loader::{load_data_dir, load_external_ids};
use taxonomy::validator::validate;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: taxonomy-validate <data-dir> <snapshot-file>");
        return ExitCode::from(2);
    }

    match run(Path::new(&args[0]), Path::new(&args[1])) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("error: {error}");
            }
            eprintln!("taxonomy validation failed with {} error(s)", errors.len());
            ExitCode::FAILURE
        }
    }
}

fn run(data_dir: &Path, snapshot: &Path) -> Result<String, Vec<TaxonomyError>> {
    let taxonomy = load_data_dir(data_dir).map_err(|error| vec![error])?;
    let external_ids: HashSet<String> = load_external_ids(snapshot).map_err(|error| vec![error])?;
    let errors = validate(&taxonomy, Some(&external_ids));
    if errors.is_empty() {
        Ok(format!(
            "taxonomy OK: {} event(s), {} category(ies), {} synonym(s), {} external id(s)",
            taxonomy.events.len(),
            taxonomy.categories.len(),
            taxonomy.synonyms.len(),
            external_ids.len()
        ))
    } else {
        Err(errors)
    }
}
