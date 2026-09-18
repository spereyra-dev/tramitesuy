//! Task 57 (SE-1, D-5): the ingestion crate's boundary guard extended from
//! task 4 — no `sqlx`/`reqwest` anywhere in `crates/ingestion` except the
//! future `ckan.rs` real fetcher — plus row-order permutation invariance of
//! the whole pipeline output.

use ingestion::format::csv::CsvStrategy;
use ingestion::pipeline::run;
use ingestion::ports::{DatasetManifest, FormatStrategy};
use support::InMemoryRepo;

mod support;

use support::FixtureFetcher;

const FORBIDDEN: [&str; 2] = ["sqlx", "reqwest"];

fn crate_dir() -> String {
    env!("CARGO_MANIFEST_DIR").to_string()
}

#[test]
fn no_sqlx_or_reqwest_outside_ckan() {
    // Manifest guard: `sqlx` never appears; `reqwest` is permitted only as an
    // OPTIONAL dependency (the `live-ckan` feature, task 62) so default
    // builds stay network-free (D-5).
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", crate_dir()))
        .expect("crate manifest readable");
    assert!(
        !manifest.contains("sqlx"),
        "crates/ingestion/Cargo.toml must not depend on 'sqlx'"
    );
    if manifest.contains("reqwest") {
        let reqwest_line = manifest
            .lines()
            .find(|line| line.trim_start().starts_with("reqwest"))
            .expect("a 'reqwest' mention must be a dependency line");
        assert!(
            reqwest_line.contains("optional = true"),
            "reqwest must stay an OPTIONAL dependency (feature live-ckan, ckan.rs exception) so \
             default builds stay network-free: {reqwest_line}"
        );
    }

    // Source guard: no forbidden token outside `ckan.rs` (the real fetcher —
    // the one sanctioned network exception, task 62/65), after stripping
    // comment lines.
    let src = format!("{}/src", crate_dir());
    let mut files = Vec::new();
    collect_rs_files(std::path::Path::new(&src), &mut files);
    assert!(
        !files.is_empty(),
        "the source scan must actually visit files"
    );
    for file in files {
        if file.ends_with("ckan.rs") {
            // Sanctioned exception: the real CKAN fetcher, feature-gated.
            continue;
        }
        let content = std::fs::read_to_string(&file).expect("source readable");
        let stripped: String = content
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for dep in FORBIDDEN {
            assert!(
                !stripped.contains(dep),
                "{} must not reference '{dep}': the ingestion crate persists only \
                 through the ProcedureRepository port and fetches only through the \
                 SourceFetcher port (D-5)",
                file.display()
            );
        }
    }
}

fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("src dir readable")
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn pipeline_output_is_invariant_under_row_order_permutation() {
    // Same input rows in a different order must yield the identical summary
    // and the identical repository state (SE-1 applied to ingestion output).
    for name in ["tramites_duplicate_ids.csv", "tramites_pipeline.csv"] {
        let bytes = fixture(name);
        let reversed = reversed_bytes(&bytes);

        let (summary_forward, repo_forward) = run_once(&bytes);
        let (summary_reversed, repo_reversed) = run_once(&reversed);

        assert_eq!(
            summary_forward.report(),
            summary_reversed.report(),
            "summary must not depend on fixture row order ({name})"
        );
        assert_eq!(
            repo_forward.state_report(),
            repo_reversed.state_report(),
            "persisted state must not depend on fixture row order ({name})"
        );
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} must be committed"))
}

fn reversed_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut rows = CsvStrategy.parse(bytes).expect("fixture parses");
    rows.reverse();
    let mut writer = csv::Writer::from_writer(Vec::new());
    if let Some(first) = rows.first() {
        let header: Vec<String> = first.column_names().map(|c| c.to_string()).collect();
        writer.write_record(&header).expect("header record writes");
    }
    for row in &rows {
        let values: Vec<String> = row
            .column_names()
            .map(|c| row.get(c).unwrap_or_default().to_string())
            .collect();
        writer.write_record(&values).expect("row record writes");
    }
    writer.into_inner().expect("csv buffer flushes")
}

fn run_once(bytes: &[u8]) -> (ingestion::summary::RunSummary, InMemoryRepo) {
    let fetcher = FixtureFetcher {
        manifest: DatasetManifest {
            resource_id: "00000000-0000-0000-0000-000000000000".to_string(),
            last_modified: "2026-09-17T00:00:00Z".to_string(),
            hash: "fixture-sha".to_string(),
        },
        bytes: bytes.to_vec(),
    };
    let repo = InMemoryRepo::default();
    let summary = run(
        &fetcher,
        &CsvStrategy,
        &repo,
        "2026-09-18T03:00:00Z".to_string(),
    )
    .expect("permutation run completes");
    (summary, repo)
}
