//! YAML loader (TX-1, D-5): reads the taxonomy directory layout
//! (`events/`, `categories/`, `synonyms/`) into the typed model, keeping
//! file provenance so validator messages can name the offending file.
//! These entry points are the crate's only filesystem surface.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::error::TaxonomyError;
use crate::model::{
    Category, CategorySource, Event, EventSource, SynonymFile, SynonymSource, Taxonomy,
};

/// Loads every YAML file under `<data_dir>/{events,categories,synonyms}/`.
/// Files are read in sorted order so the loaded vector is deterministic.
/// Parse failures are hard errors naming the file; cross-file checks
/// (duplicates, references, orphans) belong to the validator.
pub fn load_data_dir(data_dir: &Path) -> Result<Taxonomy, TaxonomyError> {
    Ok(Taxonomy {
        events: load_events(data_dir)?,
        categories: load_categories(data_dir)?,
        synonyms: load_synonyms(data_dir)?,
    })
}

/// Loads the external-id snapshot (D-2): one external id per line, sorted,
/// LF line endings. Used by the orphan check.
pub fn load_external_ids(snapshot: &Path) -> Result<HashSet<String>, TaxonomyError> {
    let text = fs::read_to_string(snapshot).map_err(|source| TaxonomyError::Io {
        path: snapshot.display().to_string(),
        source,
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn load_events(data_dir: &Path) -> Result<Vec<EventSource>, TaxonomyError> {
    let mut events = Vec::new();
    for (file, text) in read_yaml_dir(&data_dir.join("events"), "events")? {
        let event: Event = parse(&file, &text)?;
        events.push(EventSource { file, event });
    }
    Ok(events)
}

fn load_categories(data_dir: &Path) -> Result<Vec<CategorySource>, TaxonomyError> {
    let mut categories = Vec::new();
    for (file, text) in read_yaml_dir(&data_dir.join("categories"), "categories")? {
        let category: Category = parse(&file, &text)?;
        categories.push(CategorySource { file, category });
    }
    Ok(categories)
}

fn load_synonyms(data_dir: &Path) -> Result<Vec<SynonymSource>, TaxonomyError> {
    let mut synonyms = Vec::new();
    for (file, text) in read_yaml_dir(&data_dir.join("synonyms"), "synonyms")? {
        let parsed: SynonymFile = parse(&file, &text)?;
        for synonym in parsed.synonyms {
            synonyms.push(SynonymSource {
                file: file.clone(),
                synonym,
            });
        }
    }
    Ok(synonyms)
}

/// Reads every `.yaml`/`.yml` file in `dir`, sorted by file name. The file
/// label is relative to the data directory (e.g. `events/foo.yaml`) so
/// contributor-facing errors point at the real seed path.
fn read_yaml_dir(dir: &Path, label: &str) -> Result<Vec<(String, String)>, TaxonomyError> {
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| TaxonomyError::Io {
        path: dir.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| TaxonomyError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();

    let mut files = Vec::new();
    for name in names {
        let path = dir.join(&name);
        if !path.is_file() {
            continue;
        }
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("yaml") | Some("yml")) {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|source| TaxonomyError::Io {
            path: path.display().to_string(),
            source,
        })?;
        files.push((format!("{label}/{name}"), text));
    }
    Ok(files)
}

fn parse<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<T, TaxonomyError> {
    serde_yaml::from_str(text).map_err(|error| TaxonomyError::Parse {
        file: file.to_string(),
        message: error.to_string(),
    })
}
