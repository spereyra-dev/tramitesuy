//! `ingest ingest` (task 64): composes the real CKAN fetcher with the sqlx
//! repository and runs the fixture-proven pipeline from `crates/ingestion`
//! (D-5: the binary holds no pipeline logic). The run summary prints to
//! stdout — the deterministic artifact contract from task 61.
//!
//! Configuration is environment-only (IN-2): no URL literal may exist in
//! source, so the catalog base URL MUST come from `CKAN_BASE_URL`.

use crate::exclusion::IngestionExclusion;
use crate::support;
use ingestion::pipeline;
use ingestion::summary::RunStamp;

pub fn run() {
    match run_once() {
        Ok(()) => {}
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
}

/// One ingestion pass: build the fetcher, acquire the ingestion exclusion
/// (task 35: manual runs share the scheduled runs' exclusion — a blocked
/// run records a `skipped` status and is not queued), run the pipeline,
/// print the deterministic run summary. Returns a message on
/// configuration or run failure so the daemon loop can log and retry
/// without exiting.
pub fn run_once() -> Result<(), String> {
    let base = std::env::var("CKAN_BASE_URL")
        .unwrap_or_default()
        .trim()
        .to_string();
    run_once_with_base(&base)
}

/// The production-path alias: the database URL arrives from the
/// environment (IN-2).
pub fn run_once_with_base(base: &str) -> Result<(), String> {
    run_once_with_url(base, None)
}

/// One ingestion pass over an explicit catalog base URL and an explicit
/// database URL (None = the environment chain, IN-2; the failure-injection
/// and exclusion tests drive both explicitly). The ingestion exclusion is
/// held for the whole pass on the guard's transaction: the acquisition and
/// the release both run on the shared runtime (the transaction-scoped lock
/// rolls back with the guard), while the pipeline's blocking HTTP client
/// stays off async workers.
pub fn run_once_with_url(base: &str, database_url: Option<&str>) -> Result<(), String> {
    let acquired = support::block_on(async {
        let pool = support::connect_pool(&support::database_url(database_url));
        let held = IngestionExclusion::try_acquire(&pool).await;
        let held = match held {
            Ok(held) => held,
            Err(error) => return Err(format!("ingestion exclusion check failed: {error}")),
        };
        match held {
            None => {
                // The exclusion is held by another run (scheduled or
                // manual): this run terminates, is recorded `skipped`,
                // and is not queued — the API keeps serving the current
                // generation throughout.
                let recorded = crate::run_records::record_terminal_run(
                    &pool,
                    "manual",
                    "skipped",
                    serde_json::json!({ "reason": "ingestion exclusion held by another run" }),
                    1,
                )
                .await;
                let run_id = recorded.map_err(|error| {
                    format!("the skipped run record could not be written: {error}")
                })?;
                Ok(Acquired::Skipped(run_id))
            }
            Some(exclusion) => Ok(Acquired::Held(pool, exclusion)),
        }
    });
    match acquired? {
        Acquired::Skipped(run_id) => {
            eprintln!(
                "ingest skipped: the ingestion exclusion is held by another run (recorded run {run_id})"
            );
            Ok(())
        }
        Acquired::Held(pool, exclusion) => {
            let ran = run_pass_on_pool(&pool, base);
            // The exclusion is released on EVERY exit path (pipeline error
            // included) and the release runs on the shared runtime, where
            // the transaction-scoped lock rolls back with the guard.
            support::block_on(async {
                drop(exclusion);
            });
            ran
        }
    }
}

/// The ingestion pass over a pool the caller connected and an exclusion it
/// already holds (the daemon's scheduled cycle path): fetch → process →
/// dual-write, with the deterministic run summary printed.
pub fn run_pass_on_pool(pool: &sqlx::PgPool, base: &str) -> Result<(), String> {
    let fetcher = build_fetcher_with(base)?;
    let repo = support::open_repository(pool.clone());
    let summary =
        pipeline::run_csv(&fetcher, &repo, now_stamp()).map_err(|error| error.to_string())?;
    print!("{}", summary.report());
    Ok(())
}

/// The outcome of the exclusion acquisition phase of one pass.
enum Acquired {
    /// The exclusion is held by another run; the skipped run record is
    /// written and the run is not queued.
    Skipped(uuid::Uuid),
    /// This run holds the exclusion for the rest of the pass.
    Held(sqlx::PgPool, IngestionExclusion),
}

fn build_fetcher_with(
    base: &str,
) -> Result<ingestion::ckan::CkanFetcher<ingestion::ckan::ReqwestTransport>, String> {
    let base = base.trim();
    if base.is_empty() {
        // IN-2: no URL may be hardcoded in source — the caller configures it.
        return Err(
            "CKAN_BASE_URL is not set; the catalog base URL arrives from \
                    configuration (no URL literals exist in code, IN-2)"
                .to_string(),
        );
    }
    Ok(ingestion::ckan::CkanFetcher::new(
        base,
        "agesic-guia-de-tramites",
    ))
}

fn now_stamp() -> RunStamp {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        // Justified: a wall clock before the UNIX epoch cannot occur on any
        // supported platform; there is no meaningful typed-error caller to
        // propagate to on this boundary.
        .expect("system clock");
    // RFC 3339 from the epoch seconds; the repository boundary parses it.
    format_rfc3339(now.as_secs())
}

fn format_rfc3339(epoch_secs: u64) -> String {
    // Deterministic civil-from-days conversion (no chrono dependency at this
    // boundary; the port takes an RFC 3339 string).
    let days = epoch_secs / 86_400;
    let secs_of_day = epoch_secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's civil_from_days algorithm (public domain).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
