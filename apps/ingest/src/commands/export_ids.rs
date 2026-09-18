//! `ingest export-ids` (task 67, D-2): writes every ingested external_id to
//! the committed snapshot file — one per line, sorted, LF line endings,
//! trailing newline — so the taxonomy orphan check stays DB-free in CI.

use crate::support;
use ingestion::ports::ProcedureRepository;
use std::io::Write;

/// Renders the snapshot bytes and the true external-id count: sorted ids,
/// one per line, LF, trailing newline; byte-stable for the same id set (D-2).
/// The count is the post-sort/dedup number of ids contained in the returned
/// snapshot bytes (D-F1a) — not the byte length of the file.
pub fn render(mut ids: Vec<String>) -> (usize, Vec<u8>) {
    ids.sort();
    ids.dedup();
    let count = ids.len();
    let mut out = Vec::new();
    for id in ids {
        out.extend_from_slice(id.as_bytes());
        out.push(b'\n');
    }
    (count, out)
}

pub fn run(output: &str, database_url: Option<&str>) {
    let repo = support::repository_for(database_url);
    let ids = repo
        .all_external_ids()
        .expect("external ids read from the database");
    let (count, bytes) = render(ids);
    let mut file =
        std::fs::File::create(output).unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
    file.write_all(&bytes)
        .unwrap_or_else(|e| panic!("cannot write {output}: {e}"));
    println!("exported {count} external id(s) to {output}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_counts_deduplicated_ids_and_keeps_bytes_stable() {
        // Dedup contract at the render level: DB-level duplicates are
        // impossible (procedures.external_id is unique), so this is
        // render-level defense for the count contract.
        assert_eq!(
            render(vec!["b".to_string(), "a".to_string(), "b".to_string()]),
            (2, b"a\nb\n".to_vec()),
        );
    }
}
