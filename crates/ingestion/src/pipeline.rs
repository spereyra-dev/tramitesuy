//! The fixture-driven ingestion pipeline (design §4.1): resolve → download
//! → parse → validate → dedup → normalize → hash → diff → persist, entirely
//! behind ports so unit tests run offline against a `FixtureFetcher` and an
//! in-memory repository (spec IN-1, D-5).

use crate::dedup::dedup;
use crate::error::IngestionError;
use crate::format::csv::CsvStrategy;
use crate::ports::{FormatStrategy, ProcedureRepository, SourceFetcher};
use crate::row::validate_rows;
use crate::summary::{ProcedureUpsert, RunStamp, RunSummary, UpsertCounts};
use sha2::{Digest, Sha256};

/// Runs one full ingestion pass and returns the deterministic summary.
/// Validation findings (skips, duplicates) are warnings on the summary;
/// only structural failures (fetch, parse, repo) are hard errors.
pub fn run(
    fetcher: &dyn SourceFetcher,
    format: &dyn FormatStrategy,
    repo: &dyn ProcedureRepository,
    now: RunStamp,
) -> Result<RunSummary, IngestionError> {
    let mut summary = RunSummary::default();

    // resolve (IN-2: package_show at call time, in the real adapter).
    let manifest = fetcher.resolve_dataset()?;
    // download
    let bytes = fetcher.download_resource(&manifest.resource_id)?;
    // parse
    let rows = format.parse(&bytes)?;
    summary.rows_read = rows.len();
    // validate (skip-and-report, IN-4)
    let (valid, skipped) = validate_rows(rows);
    summary.record_skips(&skipped);
    // dedup (IN-5)
    let outcome = dedup(valid);
    summary.record_duplicates(outcome.warnings);
    // normalize + hash + diff vs latest known hashes (IN-6)
    let latest = repo.latest_hashes()?;
    let mut upserts = Vec::new();
    for winner in &outcome.winners {
        let external_id = winner.get("id").unwrap_or_default().to_string();
        let payload = winner.to_raw_data_json();
        let payload_json = serde_json::to_string(&payload).map_err(|e| {
            IngestionError::from(crate::error::ParseError::Malformed(e.to_string()))
        })?;
        let content_hash = digest_hex(payload_json.as_bytes());
        match latest.get(&external_id) {
            Some(existing) if existing == &content_hash => {}
            _ => {
                upserts.push(ProcedureUpsert {
                    external_id,
                    name: winner.get("nombre_tramite").unwrap_or_default().to_string(),
                    description: winner.get("ques_es").unwrap_or_default().to_string(),
                    organization_external_id: winner
                        .get("institucion_oid")
                        .unwrap_or_default()
                        .to_string(),
                    organization_name: winner
                        .get("institucion_nombre")
                        .unwrap_or_default()
                        .to_string(),
                    official_url: winner.get("url").unwrap_or_default().to_string(),
                    content_hash,
                    raw_data: payload,
                });
            }
        }
    }
    // persist (storage-agnostic through the port; single tx per batch in B4)
    let counts: UpsertCounts = repo.upsert_procedures(&upserts)?;
    summary.created += counts.inserted;
    summary.updated += counts.updated;
    // touch last_seen for every surviving row (IN-9 groundwork)
    let mut seen: Vec<String> = outcome
        .winners
        .iter()
        .map(|w| w.get("id").unwrap_or_default().to_string())
        .collect();
    seen.sort();
    repo.touch_last_seen(&seen, now)?;

    Ok(summary)
}

/// SHA-256 hex of a byte slice.
fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Convenience: run with the default CSV strategy.
pub fn run_csv(
    fetcher: &dyn SourceFetcher,
    repo: &dyn ProcedureRepository,
    now: RunStamp,
) -> Result<RunSummary, IngestionError> {
    run(fetcher, &CsvStrategy, repo, now)
}
