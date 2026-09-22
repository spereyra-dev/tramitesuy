//! `ingest ingest` (task 64): composes the real CKAN fetcher with the sqlx
//! repository and runs the fixture-proven pipeline from `crates/ingestion`
//! (D-5: the binary holds no pipeline logic). The run summary prints to
//! stdout — the deterministic artifact contract from task 61.
//!
//! Configuration is environment-only (IN-2): no URL literal may exist in
//! source, so the catalog base URL MUST come from `CKAN_BASE_URL`.

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

/// One ingestion pass: build the fetcher, run the pipeline, print the
/// deterministic run summary. Returns a message on configuration or run
/// failure so the daemon loop can log and retry without exiting.
pub fn run_once() -> Result<(), String> {
    let base = std::env::var("CKAN_BASE_URL")
        .unwrap_or_default()
        .trim()
        .to_string();
    run_once_with_base(&base)
}

/// One ingestion pass over an explicit catalog base URL (the failure-
/// injection tests drive it directly; the production path reads
/// `CKAN_BASE_URL` — IN-2: no URL literals in code).
pub fn run_once_with_base(base: &str) -> Result<(), String> {
    let fetcher = build_fetcher_with(base)?;
    let repo = support::repository_for(None);
    let summary =
        pipeline::run_csv(&fetcher, &repo, now_stamp()).map_err(|error| error.to_string())?;
    print!("{}", summary.report());
    Ok(())
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
