//! `ingest export-ids` (task 67, D-2): writes every ingested external_id to
//! the committed snapshot file — one per line, sorted, LF line endings,
//! trailing newline — so the taxonomy orphan check stays DB-free in CI.

use crate::support;
use ingestion::ports::ProcedureRepository;
use std::io::Write;

/// Renders the snapshot bytes: sorted ids, one per line, LF, trailing
/// newline. Deterministic and byte-stable for the same id set (D-2).
pub fn render(mut ids: Vec<String>) -> Vec<u8> {
    ids.sort();
    ids.dedup();
    let mut out = Vec::new();
    for id in ids {
        out.extend_from_slice(id.as_bytes());
        out.push(b'\n');
    }
    out
}

pub fn run(output: &str, database_url: Option<&str>) {
    let repo = support::repository_for(database_url);
    let ids = repo
        .all_external_ids()
        .expect("external ids read from the database");
    let bytes = render(ids);
    let mut file =
        std::fs::File::create(output).unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
    file.write_all(&bytes)
        .unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
    println!("exported {} external id(s) to {output}", bytes.len());
}
