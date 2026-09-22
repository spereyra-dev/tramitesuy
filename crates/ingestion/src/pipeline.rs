//! The fixture-driven ingestion pipeline (design §4.1): resolve → download
//! → parse → validate → dedup → normalize → hash → diff → persist, entirely
//! behind ports so unit tests run offline against a `FixtureFetcher` and the
//! canonical in-memory repository (spec IN-1, D-5).

use crate::dedup::dedup;
use crate::diff;
use crate::error::IngestionError;
use crate::format::csv::CsvStrategy;
use crate::ports::{FormatStrategy, ProcedureRepository, SourceFetcher};
use crate::row::validate_rows;
use crate::summary::{RunStamp, RunSummary};
use std::collections::BTreeSet;

/// Runs one full ingestion pass and returns the deterministic summary.
/// Validation findings (skips, duplicates) are warnings on the summary;
/// only structural failures (fetch, parse, repo) are hard errors. A batch
/// that yields no valid row is such a structural failure
/// ([`IngestionError::EmptyBatch`]): the run returns before touching state,
/// because `deactivate_missing` with an empty present set would deactivate
/// the entire catalog.
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
    // dedup (IN-5): winners only; losers land in the summary's row accounting
    let outcome = dedup(valid);
    summary.record_duplicates(outcome.warnings, outcome.resolved_rows);

    // An empty or fully invalid batch must change nothing. `deactivate_missing`
    // treats every known id absent from `present` as missing, so with an empty
    // `present` set it would soft-delete the whole catalog. The guard therefore
    // sits before every state-changing repository call (`upsert_procedures`,
    // `close_versions`, `deactivate_missing`, `touch_last_seen`).
    if outcome.winners.is_empty() {
        return Err(IngestionError::EmptyBatch {
            rows_read: summary.rows_read,
            skipped: summary.rows_skipped,
        });
    }

    // normalize + hash + diff vs latest known hashes (IN-6, DM-3).
    let latest = repo.latest_hashes()?;
    let plan = diff::plan(&outcome.winners, &latest)?;

    // persist (storage-agnostic through the port; single tx per batch in B4):
    // new and changed rows upsert exactly one new version each.
    let counts = repo.upsert_procedures(&plan.upserts, now.clone())?;
    summary.created += counts.inserted;
    summary.updated += counts.updated;
    summary.unchanged += plan.unchanged.len();
    // close the prior open version of every changed row at the run stamp
    // (DM-3: valid_until is the only post-insert write, only on the prior
    // open version).
    if !plan.closes.is_empty() {
        repo.close_versions(&plan.closes, now.clone())?;
    }
    // soft delete (IN-7): rows absent from the source become inactive with
    // deactivated_at stamped, never deleted. The empty-batch guard above only
    // covers the fully empty / fully invalid batch (empty `present`); a
    // non-empty partial batch still deactivates every absent row by design, so
    // the publication validation gate is the second line of defence against a
    // mass deactivation.
    let present: BTreeSet<String> = outcome
        .winners
        .iter()
        .map(|winner| winner.get("id").unwrap_or_default().to_string())
        .collect();
    summary.deactivated += repo.deactivate_missing(&present, now.clone())?;
    // touch last_seen for every surviving row (IN-7, IN-9); BTreeSet order
    // keeps the call row-order invariant (SE-1).
    let seen: Vec<String> = present.into_iter().collect();
    // A row present in the current source is active again, independent of the
    // diff classification (F12): the `unchanged` path advances `last_seen_at`
    // only, so a procedure re-published byte-identical after a deactivation
    // would otherwise stay inactive forever. Runs over the same present set as
    // `touch_last_seen` (disjoint from `deactivate_missing`'s absent set) and
    // opens no version.
    repo.reactivate_present(&seen, now.clone())?;
    repo.touch_last_seen(&seen, now)?;
    // canonical warning order keeps the summary permutation-invariant (SE-1).
    summary.canonicalize_warnings();

    Ok(summary)
}

/// Convenience: run with the default CSV strategy.
pub fn run_csv(
    fetcher: &dyn SourceFetcher,
    repo: &dyn ProcedureRepository,
    now: RunStamp,
) -> Result<RunSummary, IngestionError> {
    run(fetcher, &CsvStrategy, repo, now)
}
