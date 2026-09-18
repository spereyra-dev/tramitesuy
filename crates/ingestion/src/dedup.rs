//! Duplicate external_id winner rule (spec IN-5, design §3):
//! 1. the row with the most recent `actualizado` wins;
//! 2. on an exact timestamp tie, the row whose raw serialization has the
//!    lexicographically greater SHA-256 hex digest wins.
//!
//! The rule is applied identically every run — same input ⇒ same winner —
//! and the outcome is a `RunWarning`, never a hard error.

use crate::row::RawRow;
use crate::summary::RunWarning;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Result of duplicate resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupOutcome {
    /// One winner per unique id, sorted by id (row-order invariant).
    pub winners: Vec<RawRow>,
    /// One warning per resolved duplicate id, sorted by id.
    pub warnings: Vec<RunWarning>,
}

/// Sort key for the winner rule: (timestamp digits, digest hex, raw
/// serialization). The trailing serialization keeps the ordering total and
/// permutation-invariant even for byte-identical rows.
fn winner_key(row: &RawRow) -> (u128, String, String) {
    (
        timestamp_key(row.get("actualizado")),
        digest_hex(row),
        row.raw_serialization(),
    )
}

/// Extracts the digit characters of a timestamp into a fixed-width-ish
/// numeric key. For well-formed `YYYY-MM-DD[T]HH:MM[:SS]` values this is
/// monotone with chronological order; malformed values collapse to 0 —
/// deterministic either way.
fn timestamp_key(value: Option<&str>) -> u128 {
    let digits: String = value
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

fn digest_hex(row: &RawRow) -> String {
    let bytes: Vec<u8> = Sha256::digest(row.raw_serialization().as_bytes()).to_vec();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn describe(row: &RawRow) -> String {
    format!(
        "actualizado={}, sha256={}",
        row.get("actualizado").unwrap_or_default(),
        digest_hex(row)
    )
}

/// Groups rows by `id` and applies the deterministic winner rule per group
/// (IN-5). Output order and warnings are deterministic for any row order.
pub fn dedup(rows: Vec<RawRow>) -> DedupOutcome {
    let mut groups: BTreeMap<String, Vec<RawRow>> = BTreeMap::new();
    for row in rows {
        let id = row.get("id").unwrap_or_default().to_string();
        groups.entry(id).or_default().push(row);
    }

    let mut winners = Vec::new();
    let mut warnings = Vec::new();
    for (id, mut group) in groups {
        group.sort_by_key(winner_key);
        let winner = group.pop().expect("group is never empty");
        if !group.is_empty() {
            let mut losers: Vec<String> = group.iter().map(describe).collect();
            losers.sort();
            warnings.push(RunWarning::DuplicateId {
                id,
                winner: describe(&winner),
                losers,
            });
        }
        winners.push(winner);
    }

    DedupOutcome { winners, warnings }
}
