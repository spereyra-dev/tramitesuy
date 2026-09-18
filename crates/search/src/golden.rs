//! Golden-dataset evaluation harness runner (SE-12, D-7, tasks 34–38).
//!
//! Pure module: it receives the parsed dataset (`parse_dataset`), the
//! already-loaded engine, and a provider slice; it never touches the
//! filesystem (the test binary reads `tests/search/golden_dataset.yaml`,
//! and the taxonomy loader feeds the real seed in). For every case it
//! records per-case expectation failures naming the query, then reports
//! the Top1 / Top3 / no-result / ambiguous metrics table and gates the
//! recorded baselines so a ranking regression fails `cargo test`.

use serde::Deserialize;

use crate::engine::{CandidateProvider, SearchEngine};
use crate::types::{SearchOutcome, SelectionMode};

/// Golden dataset file format (design D-7).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenDataset {
    pub version: u32,
    pub baselines: Baselines,
    #[serde(default)]
    pub cases: Vec<GoldenCase>,
}

/// Recorded non-regression baselines gating the metrics (D-7).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baselines {
    pub top1: f64,
    pub top3: f64,
    pub max_no_result_rate: f64,
    pub max_ambiguous_rate: f64,
}

/// One golden case: a query and its expected outcome. Expectations are
/// optional so ambiguity and no-result scenarios can be expressed
/// without forcing a wrong top1.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenCase {
    pub query: String,
    #[serde(default)]
    pub expect_top1: Option<String>,
    #[serde(default)]
    pub expect_top3: Vec<String>,
    #[serde(default)]
    pub expect_not_top1: Vec<String>,
}

/// Per-case run outcome: the selection band the engine landed in, the
/// actual top1 slug, the expectation bookkeeping the metrics aggregate,
/// and every expectation failure naming the query.
#[derive(Debug, Clone, PartialEq)]
pub struct CaseOutcome {
    pub query: String,
    pub mode: Option<SelectionMode>,
    pub top1_slug: Option<String>,
    pub top1_declared: bool,
    pub top1_hit: bool,
    pub top3_declared: bool,
    pub top3_hit: bool,
    pub failures: Vec<String>,
}

/// Dataset-level metrics over the run outcomes (SE-12 "metrics are
/// reported per run").
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub total_cases: usize,
    pub top1_cases: usize,
    pub top1_hits: usize,
    pub top3_cases: usize,
    pub top3_hits: usize,
    pub no_result_cases: usize,
    pub ambiguous_cases: usize,
}

/// Parses the golden dataset YAML, rejecting unknown fields and unknown
/// dataset versions so a malformed file fails loudly instead of running.
pub fn parse_dataset(yaml: &str) -> Result<GoldenDataset, String> {
    let dataset: GoldenDataset =
        serde_yaml::from_str(yaml).map_err(|error| format!("invalid golden dataset: {error}"))?;
    if dataset.version != 1 {
        return Err(format!(
            "unsupported golden dataset version {}, expected 1",
            dataset.version
        ));
    }
    Ok(dataset)
}

/// Runs every dataset case over the engine and collects per-case
/// expectation failures naming the regressing query.
pub fn run_cases(
    engine: &SearchEngine,
    providers: &[&dyn CandidateProvider],
    dataset: &GoldenDataset,
) -> Vec<CaseOutcome> {
    dataset
        .cases
        .iter()
        .map(|case| run_case(engine, providers, case))
        .collect()
}

fn run_case(
    engine: &SearchEngine,
    providers: &[&dyn CandidateProvider],
    case: &GoldenCase,
) -> CaseOutcome {
    let outcome = match engine.search(&case.query, providers) {
        Ok(outcome) => outcome,
        Err(error) => {
            return CaseOutcome {
                query: case.query.clone(),
                mode: None,
                top1_slug: None,
                top1_declared: case.expect_top1.is_some(),
                top1_hit: false,
                top3_declared: !case.expect_top3.is_empty(),
                top3_hit: false,
                failures: vec![format!("engine error: {error}")],
            };
        }
    };
    let failures = case_failures(&outcome, case);
    let top1_hit = match (&case.expect_top1, outcome.results.first()) {
        (Some(expected), Some(actual)) => actual.slug == *expected,
        _ => false,
    };
    let top3_declared = !case.expect_top3.is_empty();
    let top3_hit = top3_declared
        && case.expect_top3.iter().all(|slug| {
            outcome
                .results
                .iter()
                .take(3)
                .any(|result| result.slug == *slug)
        });
    CaseOutcome {
        query: case.query.clone(),
        mode: Some(outcome.selection.mode),
        top1_slug: outcome.results.first().map(|result| result.slug.clone()),
        top1_declared: case.expect_top1.is_some(),
        top1_hit,
        top3_declared,
        top3_hit,
        failures,
    }
}

fn case_failures(outcome: &SearchOutcome, case: &GoldenCase) -> Vec<String> {
    let mut failures = Vec::new();
    let top1 = outcome.results.first().map(|result| result.slug.as_str());
    if let Some(expected) = &case.expect_top1
        && top1 != Some(expected.as_str())
    {
        failures.push(format!(
            "expected top1 {expected}, got {}",
            top1.unwrap_or("<no results>")
        ));
    }
    for slug in &case.expect_not_top1 {
        if top1 == Some(slug.as_str()) {
            failures.push(format!("expected {slug} NOT to rank top1"));
        }
    }
    if !case.expect_top3.is_empty() {
        let top3: Vec<&str> = outcome
            .results
            .iter()
            .take(3)
            .map(|result| result.slug.as_str())
            .collect();
        for slug in &case.expect_top3 {
            if !top3.contains(&slug.as_str()) {
                failures.push(format!(
                    "expected {slug} within the top 3 results, got {top3:?}"
                ));
            }
        }
    }
    failures
}

/// Computes the dataset metrics: Top1/Top3 accuracy over the cases that
/// declare the corresponding expectation, and the no-result / ambiguous
/// rates over every case (SE-12, task 37 accounting).
pub fn compute_metrics(outcomes: &[CaseOutcome]) -> Metrics {
    Metrics {
        total_cases: outcomes.len(),
        top1_cases: outcomes.iter().filter(|o| o.top1_declared).count(),
        top1_hits: outcomes.iter().filter(|o| o.top1_hit).count(),
        top3_cases: outcomes.iter().filter(|o| o.top3_declared).count(),
        top3_hits: outcomes.iter().filter(|o| o.top3_hit).count(),
        no_result_cases: outcomes
            .iter()
            .filter(|o| o.mode == Some(SelectionMode::Categories))
            .count(),
        ambiguous_cases: outcomes
            .iter()
            .filter(|o| o.mode == Some(SelectionMode::Disambiguation))
            .count(),
    }
}

impl Metrics {
    /// Top1 accuracy; vacuously 1.0 when no case declares an expectation.
    pub fn top1_rate(&self) -> f64 {
        rate(self.top1_hits, self.top1_cases)
    }

    /// Top3 accuracy; vacuously 1.0 when no case declares expect_top3.
    pub fn top3_rate(&self) -> f64 {
        rate(self.top3_hits, self.top3_cases)
    }

    /// Share of cases that fell to the related-categories no-result path.
    pub fn no_result_rate(&self) -> f64 {
        rate(self.no_result_cases, self.total_cases)
    }

    /// Share of cases that landed in the disambiguation band.
    pub fn ambiguous_rate(&self) -> f64 {
        rate(self.ambiguous_cases, self.total_cases)
    }

    /// The printable metrics table reported on every harness run.
    pub fn metrics_table(&self) -> String {
        format!(
            "golden dataset metrics ({} case(s)):\n\
             | metric          | value | cases  |\n\
             |-----------------|-------|--------|\n\
             | Top1 accuracy   | {:.2}  | {}/{}  |\n\
             | Top3 accuracy   | {:.2}  | {}/{}  |\n\
             | No-result rate  | {:.2}  | {}/{}  |\n\
             | Ambiguous rate  | {:.2}  | {}/{}  |",
            self.total_cases,
            self.top1_rate(),
            self.top1_hits,
            self.top1_cases,
            self.top3_rate(),
            self.top3_hits,
            self.top3_cases,
            self.no_result_rate(),
            self.no_result_cases,
            self.total_cases,
            self.ambiguous_rate(),
            self.ambiguous_cases,
            self.total_cases,
        )
    }
}

fn rate(hits: usize, cases: usize) -> f64 {
    if cases == 0 {
        return 1.0;
    }
    (hits as f64) / (cases as f64)
}

/// The golden gate (SE-12, D-7): runs every case, reports the metrics
/// table, and fails — naming the regressing query — when any case breaks
/// its expectations or a metric crosses its recorded baseline.
pub fn gate(
    engine: &SearchEngine,
    providers: &[&dyn CandidateProvider],
    dataset: &GoldenDataset,
) -> Result<Metrics, String> {
    let outcomes = run_cases(engine, providers, dataset);
    let metrics = compute_metrics(&outcomes);

    let mut failures: Vec<String> = Vec::new();
    for outcome in &outcomes {
        for failure in &outcome.failures {
            failures.push(format!("golden case {:?}: {}", outcome.query, failure));
        }
    }
    if metrics.top1_rate() < dataset.baselines.top1 {
        failures.push(format!(
            "Top1 accuracy {:.2} is below the recorded baseline {:.2}",
            metrics.top1_rate(),
            dataset.baselines.top1
        ));
    }
    if metrics.top3_rate() < dataset.baselines.top3 {
        failures.push(format!(
            "Top3 accuracy {:.2} is below the recorded baseline {:.2}",
            metrics.top3_rate(),
            dataset.baselines.top3
        ));
    }
    if metrics.no_result_rate() > dataset.baselines.max_no_result_rate {
        failures.push(format!(
            "no-result rate {:.2} exceeds the recorded maximum {:.2}",
            metrics.no_result_rate(),
            dataset.baselines.max_no_result_rate
        ));
    }
    if metrics.ambiguous_rate() > dataset.baselines.max_ambiguous_rate {
        failures.push(format!(
            "ambiguous rate {:.2} exceeds the recorded maximum {:.2}",
            metrics.ambiguous_rate(),
            dataset.baselines.max_ambiguous_rate
        ));
    }

    if failures.is_empty() {
        Ok(metrics)
    } else {
        Err(failures.join("\n"))
    }
}
