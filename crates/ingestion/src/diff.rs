//! Version diff planning (spec IN-6, DM-3): normalize each winning row into
//! its raw-data payload, hash it with SHA-256, and plan the persistence
//! batch against the repository's latest known hashes — the new/changed
//! upserts, the `(external_id, prior_hash)` pairs whose open version must
//! close, and the unchanged ids that only advance `last_seen_at`.

use crate::error::{IngestionError, ParseError};
use crate::row::RawRow;
use crate::summary::ProcedureUpsert;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// The persistence batch planned from one run's winners.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DiffPlan {
    /// New procedures and procedures whose content changed: exactly one new
    /// version each, with the prior version's `valid_until` closed (IN-6).
    pub upserts: Vec<ProcedureUpsert>,
    /// `(external_id, prior_content_hash)` pairs whose open version closes
    /// at this run's timestamp (DM-3).
    pub closes: Vec<(String, String)>,
    /// External ids whose hash matches the latest open version — no version
    /// row; only `last_seen_at` advances (IN-9).
    pub unchanged: Vec<String>,
}

/// Serializes a row's normalized payload — all source columns as a JSON
/// object (sorted keys, so the hash is row-order invariant) — destined for
/// `procedures.raw_data` JSONB.
pub fn payload_json(row: &RawRow) -> Result<String, IngestionError> {
    let payload = row.to_raw_data_json();
    serde_json::to_string(&payload)
        .map_err(|e| IngestionError::from(ParseError::Malformed(e.to_string())))
}

/// `content_hash = SHA-256(normalized_payload)` (IN-6).
pub fn content_hash(payload_json: &str) -> String {
    hex(&Sha256::digest(payload_json.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Plans the persistence batch for one run. The pipeline applies the plan
/// through the [`ProcedureRepository`](crate::ports::ProcedureRepository)
/// port; the plan itself is storage-agnostic, deterministic, and
/// row-order-invariant: every vec is ordered by external id.
pub fn plan(rows: &[RawRow], latest: &HashMap<String, String>) -> Result<DiffPlan, IngestionError> {
    let mut ordered: BTreeMap<&str, &RawRow> = BTreeMap::new();
    for row in rows {
        ordered.insert(row.get("id").unwrap_or_default(), row);
    }

    let mut plan = DiffPlan::default();
    for (external_id, row) in ordered {
        let content_hash = content_hash(payload_json(row)?.as_str());
        match latest.get(external_id) {
            Some(existing) if existing == &content_hash => {
                plan.unchanged.push(external_id.to_string());
            }
            Some(prior) => {
                plan.closes.push((external_id.to_string(), prior.clone()));
                plan.upserts.push(upsert_of(row, content_hash));
            }
            None => plan.upserts.push(upsert_of(row, content_hash)),
        }
    }
    Ok(plan)
}

fn upsert_of(row: &RawRow, content_hash: String) -> ProcedureUpsert {
    ProcedureUpsert {
        external_id: row.get("id").unwrap_or_default().to_string(),
        name: row.get("nombre_tramite").unwrap_or_default().to_string(),
        description: row.get("ques_es").unwrap_or_default().to_string(),
        organization_external_id: row.get("institucion_oid").unwrap_or_default().to_string(),
        organization_name: row
            .get("institucion_nombre")
            .unwrap_or_default()
            .to_string(),
        official_url: row.get("url").unwrap_or_default().to_string(),
        content_hash,
        raw_data: row.to_raw_data_json(),
    }
}
